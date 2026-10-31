/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;

use anyhow::{Result, ensure};
use threadpool::ThreadPool;

use crate::args::Args;
use crate::services::metadata::MetadataService;
use crate::structs::deezer::{Album, Track};
use crate::structs::youtube::Video;
use crate::utils::escape_path;

pub(crate) const DOWNLOAD_THREAD_COUNT: usize = 16;
const TRACK_DURATION_SLACK: i64 = 5;

pub(crate) enum ProgressEvent {
    AlbumQueued {
        album_id: i64,
        title: String,
        cover_small: String,
        artist_name: String,
        start_index: usize,
        track_count: usize,
    },
    Added {
        index: usize,
        label: String,
    },
    Searching {
        index: usize,
    },
    Downloading {
        index: usize,
        percent: f32,
    },
    WritingMetadata {
        index: usize,
    },
    Done {
        index: usize,
    },
    Failed {
        index: usize,
    },
}

struct TrackJob {
    album: Album,
    album_folder: PathBuf,
    album_cover: Option<Vec<u8>>,
    album_nb_disks: i64,
    track: Track,
    track_index: usize,
    global_index: usize,
    tx: mpsc::Sender<ProgressEvent>,
}

pub(crate) struct Downloader {
    pool: ThreadPool,
    track_start: usize,
}

impl Downloader {
    pub(crate) fn new() -> Self {
        Self {
            pool: ThreadPool::new_fixed(DOWNLOAD_THREAD_COUNT),
            track_start: 0,
        }
    }

    pub(crate) fn queue_album(
        &mut self,
        args: &Args,
        metadata_service: &mut MetadataService,
        album_id: i64,
        tx: mpsc::Sender<ProgressEvent>,
    ) -> Result<()> {
        let added = download_album(
            args,
            &mut self.pool,
            metadata_service,
            album_id,
            tx,
            self.track_start,
        )?;
        self.track_start += added;
        Ok(())
    }

    pub(crate) fn wait(self) {
        self.pool.join();
    }
}

fn download_album(
    args: &Args,
    pool: &mut ThreadPool,
    metadata_service: &mut MetadataService,
    album_id: i64,
    tx: mpsc::Sender<ProgressEvent>,
    track_start: usize,
) -> Result<usize> {
    let album = metadata_service.get_album(album_id)?;
    ensure!(
        !album.contributors.is_empty(),
        "Album {album_id} has no artist"
    );

    let album_folder = PathBuf::from(&args.output_dir)
        .join(escape_path(&album.contributors[0].name))
        .join(escape_path(&album.title));
    fs::create_dir_all(&album_folder)?;

    let album_cover = if let Some(album_cover_xl) = &album.cover_xl {
        let album_cover = metadata_service.download(album_cover_xl)?;
        if args.with_cover {
            fs::write(album_folder.join("cover.jpg"), &album_cover)?;
        }
        Some(album_cover)
    } else {
        None
    };

    let mut tracks = Vec::new();
    let mut album_nb_disks = 0;
    let mut previous_disk_number = 0;
    for track in &album.tracks.data {
        let track = metadata_service.get_track(track.id)?;
        if track.disk_number != previous_disk_number {
            album_nb_disks += 1;
            previous_disk_number = track.disk_number;
        }
        tracks.push(track);
    }

    tx.send(ProgressEvent::AlbumQueued {
        album_id,
        title: album.title.clone(),
        cover_small: album
            .cover_small
            .clone()
            .unwrap_or_else(|| album.cover.clone()),
        artist_name: album.contributors[0].name.clone(),
        start_index: track_start,
        track_count: tracks.len(),
    })
    .ok();

    for (local_index, track) in tracks.iter().enumerate() {
        let label = format!("{} - {}", album.contributors[0].name, track.title);
        tx.send(ProgressEvent::Added {
            index: track_start + local_index,
            label,
        })
        .ok();
    }
    let tracks_len = tracks.len();

    for (local_index, track) in tracks.into_iter().enumerate() {
        let job = TrackJob {
            album: album.clone(),
            album_folder: album_folder.clone(),
            album_cover: album_cover.clone(),
            album_nb_disks,
            track,
            track_index: local_index,
            global_index: track_start + local_index,
            tx: tx.clone(),
        };
        pool.execute(move || {
            _ = download_track(job);
        });
    }
    Ok(tracks_len)
}

fn download_track(job: TrackJob) -> Result<()> {
    let TrackJob {
        album,
        album_folder,
        album_cover,
        album_nb_disks,
        track,
        track_index,
        global_index,
        tx,
    } = job;

    let path = album_folder.join(format!(
        "{} - {} - {:0track_index_width$} - {}.m4a",
        escape_path(&album.contributors[0].name),
        escape_path(&album.title),
        track_index + 1,
        escape_path(&track.title),
        track_index_width = (album.nb_tracks as f64).log10().ceil() as usize
    ));

    let search_queries = [
        format!("{} - {}", album.contributors[0].name, track.title),
        format!(
            "{} - {} - {}",
            album.contributors[0].name, album.title, track.title
        ),
        format!("{} - {}", album.title, track.title),
    ];

    // Phase 1: collect up to 3 duration-matching video candidates across search queries
    tx.send(ProgressEvent::Searching {
        index: global_index,
    })
    .ok();
    let mut candidates: Vec<String> = Vec::new();
    for search_query in &search_queries {
        let mut search_process = Command::new("yt-dlp")
            .arg("--dump-json")
            .arg(format!("ytsearch25:{search_query}"))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdout = search_process.stdout.as_mut().expect("stdout is piped");
        for line in BufReader::new(stdout).lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let Ok(video) = serde_json::from_str::<Video>(&line) else {
                continue;
            };
            if track.duration >= video.duration - TRACK_DURATION_SLACK
                && track.duration <= video.duration + TRACK_DURATION_SLACK
                && !candidates.contains(&video.id)
            {
                candidates.push(video.id.clone());
                if candidates.len() >= 3 {
                    break;
                }
            }
        }
        search_process.kill().ok();
        search_process.wait().ok();

        if !candidates.is_empty() {
            break;
        }
    }

    // Phase 2: try each candidate until one downloads successfully
    // yt-dlp writes progress to stderr; stdout is unused for file downloads
    for video_id in &candidates {
        let mut download_process = Command::new("yt-dlp")
            .arg("--newline")
            .arg("--progress")
            .arg("-f")
            .arg("bestaudio[ext=m4a]")
            .arg(format!("https://www.youtube.com/watch?v={video_id}"))
            .arg("-o")
            // yt-dlp expands % fields in the output template
            .arg(path.to_string_lossy().replace('%', "%%"))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;

        tx.send(ProgressEvent::Downloading {
            index: global_index,
            percent: 0.0,
        })
        .ok();
        let stderr = download_process.stderr.take().expect("stderr is piped");
        for line in BufReader::new(stderr).lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Some(pct) = parse_ytdlp_percent(&line) {
                tx.send(ProgressEvent::Downloading {
                    index: global_index,
                    percent: pct,
                })
                .ok();
            }
        }

        let status = download_process.wait()?;
        if !status.success() {
            let _ = fs::remove_file(&path);
            continue;
        }

        // Update metadata
        tx.send(ProgressEvent::WritingMetadata {
            index: global_index,
        })
        .ok();
        let mut tag = mp4ameta::Tag::default();
        tag.set_title(&track.title);
        for artist in &album.contributors {
            tag.add_artist(artist.name.as_str());
        }
        for artist in &track.contributors {
            if album.contributors.iter().any(|a| a.id == artist.id) {
                continue;
            }
            tag.add_artist(artist.name.as_str());
        }
        tag.set_album(&album.title);
        for artist in &album.contributors {
            tag.add_album_artist(artist.name.as_str());
        }
        for genre in &album.genres.data {
            tag.add_genre(genre.name.as_str());
        }
        tag.set_track(track.track_position as u16, album.nb_tracks as u16);
        tag.set_disc(track.disk_number as u16, album_nb_disks as u16);
        tag.set_year(
            album
                .release_date
                .split('-')
                .next()
                .expect("Can't parse track release year"),
        );
        tag.set_bpm(track.bpm as u16);
        if let Some(album_cover) = album_cover {
            tag.set_artwork(mp4ameta::Img::jpeg(album_cover));
        }
        tag.write_to_path(&path)?;

        tx.send(ProgressEvent::Done {
            index: global_index,
        })
        .ok();
        return Ok(());
    }

    tx.send(ProgressEvent::Failed {
        index: global_index,
    })
    .ok();
    Ok(())
}

fn parse_ytdlp_percent(line: &str) -> Option<f32> {
    let rest = line.strip_prefix("[download]")?.trim();
    rest.split('%').next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ytdlp_percent() {
        assert_eq!(
            parse_ytdlp_percent("[download]   0.0% of   5.23MiB at Unknown B/s ETA Unknown"),
            Some(0.0)
        );
        assert_eq!(
            parse_ytdlp_percent("[download]  12.5% of   5.23MiB at    2.00MiB/s ETA 00:08"),
            Some(12.5)
        );
        assert_eq!(
            parse_ytdlp_percent(
                "[download]  45.6% of ~  5.23MiB at    2.00MiB/s ETA 00:02 (frag 5/10)"
            ),
            Some(45.6)
        );
        assert_eq!(
            parse_ytdlp_percent("[download] 100% of   5.23MiB at    5.00MiB/s ETA 00:00"),
            Some(100.0)
        );
        assert_eq!(
            parse_ytdlp_percent("[download] Destination: /path/to/file.m4a"),
            None
        );
        assert_eq!(
            parse_ytdlp_percent("[info] Writing video metadata as JSON to: file.json"),
            None
        );
        assert_eq!(
            parse_ytdlp_percent("ERROR: unable to download video data: HTTP Error 403: Forbidden"),
            None
        );
    }
}
