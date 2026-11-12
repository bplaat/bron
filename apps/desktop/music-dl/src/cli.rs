/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::io::{self, IsTerminal, Write};
use std::mem::DropGuard;
use std::process::exit;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::args::{Args, Subcommand};
use crate::downloader::{DOWNLOAD_THREAD_COUNT, Downloader, ProgressEvent};
use crate::services::metadata::MetadataService;
use crate::utils::{format_bar, get_album_ids, truncate};

// MARK: Renderer types
enum TrackStatus {
    Queued,
    Searching,
    Downloading(f32),
    WritingMetadata,
    Done,
    Failed,
}

struct TrackState {
    label: String,
    status: TrackStatus,
    search_start: Option<Instant>,
}

// MARK: Entry point
pub(crate) fn run(args: &Args) {
    match args.subcommand {
        Subcommand::Download => subcommand_download(args),
        Subcommand::List => subcommand_list(args),
        Subcommand::Help => subcommand_help(),
        Subcommand::Version => subcommand_version(),
    }
}

// MARK: Subcommands
fn subcommand_download(args: &Args) {
    if args.query.is_empty() {
        eprintln!("Query argument is required");
        exit(1);
    }

    let mut metadata_service = MetadataService::new();
    let album_ids = get_album_ids(&mut metadata_service, args).expect("Can't get album ids");

    let (tx, rx) = mpsc::channel::<ProgressEvent>();
    let is_tty = io::stdout().is_terminal();
    #[cfg(windows)]
    {
        _ = enable_ansi_support::enable_ansi_support();
    }
    let renderer_handle = thread::Builder::new()
        .name("progress-renderer".to_string())
        .spawn(move || run_renderer(rx, is_tty))
        .expect("Failed to spawn progress renderer thread");

    let mut downloader = Downloader::new();
    for album_id in album_ids {
        downloader
            .queue_album(args, &mut metadata_service, album_id, tx.clone())
            .expect("Can't download album");
    }

    downloader.wait();
    drop(tx);
    renderer_handle.join().expect("Renderer thread panicked");
}

fn subcommand_list(args: &Args) {
    if args.query.is_empty() {
        eprintln!("Query argument is required");
        exit(1);
    }

    let mut metadata_service = MetadataService::new();
    let album_ids = get_album_ids(&mut metadata_service, args).expect("Can't get album ids");

    for album_id in album_ids {
        let album = metadata_service
            .get_album(album_id)
            .expect("Can't get album");
        let mut tracks = Vec::new();
        let mut album_is_multi_disk = false;
        for track in &album.tracks.data {
            let track = metadata_service
                .get_track(track.id)
                .expect("Can't get track");
            if track.disk_number > 1 {
                album_is_multi_disk = true;
            }
            tracks.push(track);
        }

        println!(
            "# {} by {}\n",
            album.title,
            album
                .contributors
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "**Released at {} with {} tracks**\n",
            album.release_date, album.nb_tracks
        );

        let mut last_disk_number = 0;
        for track in &tracks {
            if album_is_multi_disk && track.disk_number != last_disk_number {
                println!(
                    "{}## Disk {}\n",
                    if track.disk_number != 1 { "\n" } else { "" },
                    track.disk_number
                );
                last_disk_number = track.disk_number;
            }
            println!(
                "{}. {} ({}:{:02}) by {}",
                track.track_position,
                track.title,
                track.duration / 60,
                track.duration % 60,
                track
                    .contributors
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!();

        thread::sleep(Duration::from_millis(500));
    }
}

fn subcommand_help() {
    println!(
        r"Usage: music-dl [SUBCOMMAND] [OPTIONS]

Options:
  -o <dir>            Change output directory
  -i, --id            Query is a Deezer ID
  -a, --artist        Query is an artist name
  -s, --with-singles  Include singles of artist
  -c, --with-cover    Also download cover image

Subcommands:
  download            Download album or artist
  list                List all albums of artist
  help                Print this help message
  version             Print the version number"
    );
}

fn subcommand_version() {
    println!("music-dl v{}", env!("CARGO_PKG_VERSION"));
}

// MARK: Renderer
fn run_renderer(rx: mpsc::Receiver<ProgressEvent>, is_tty: bool) {
    let _guard = is_tty.then(|| {
        DropGuard::new((), |()| {
            print!("\x1b[?25h");
            io::stdout().flush().ok();
        })
    });

    let mut tracks: Vec<TrackState> = Vec::new();
    let mut lines_drawn = 0usize;
    let mut done = 0usize;
    let mut failed = 0usize;

    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => {
                if let ProgressEvent::AlbumQueued { .. } = &event {
                    continue;
                }
                let updated_index = match &event {
                    ProgressEvent::Added { index, .. }
                    | ProgressEvent::Searching { index }
                    | ProgressEvent::Downloading { index, .. }
                    | ProgressEvent::WritingMetadata { index }
                    | ProgressEvent::Done { index }
                    | ProgressEvent::Failed { index } => *index,
                    ProgressEvent::AlbumQueued { .. } => unreachable!(),
                };

                if tracks.len() <= updated_index {
                    tracks.resize_with(updated_index + 1, || TrackState {
                        label: String::new(),
                        status: TrackStatus::Queued,
                        search_start: None,
                    });
                }

                match event {
                    ProgressEvent::Added { index, label } => {
                        tracks[index] = TrackState {
                            label,
                            status: TrackStatus::Queued,
                            search_start: None,
                        };
                    }
                    ProgressEvent::Searching { index } => {
                        tracks[index].status = TrackStatus::Searching;
                        tracks[index].search_start = Some(Instant::now());
                    }
                    ProgressEvent::Downloading { index, percent } => {
                        tracks[index].status = TrackStatus::Downloading(percent);
                    }
                    ProgressEvent::WritingMetadata { index } => {
                        tracks[index].status = TrackStatus::WritingMetadata;
                    }
                    ProgressEvent::Done { index } => {
                        tracks[index].status = TrackStatus::Done;
                        done += 1;
                    }
                    ProgressEvent::Failed { index } => {
                        tracks[index].status = TrackStatus::Failed;
                        failed += 1;
                    }
                    ProgressEvent::AlbumQueued { .. } => unreachable!(),
                }

                if !is_tty {
                    print_plain(&tracks[updated_index]);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if is_tty {
            redraw_tty(&tracks, &mut lines_drawn, done, failed);
        }
    }

    if failed > 0 {
        if is_tty {
            println!();
        }
        println!("--- {failed} track(s) failed to download ---");
        for (i, t) in tracks.iter().enumerate() {
            if matches!(t.status, TrackStatus::Failed) {
                println!("  [{i}] {}", t.label);
            }
        }
    }
}

fn redraw_tty(tracks: &[TrackState], lines_drawn: &mut usize, done: usize, failed: usize) {
    if *lines_drawn > 0 {
        print!("\x1b[{lines_drawn}A");
    }
    print!("\x1b[?25l");

    let active: Vec<_> = tracks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TrackStatus::Searching | TrackStatus::Downloading(_) | TrackStatus::WritingMetadata
            )
        })
        .collect();

    for i in 0..DOWNLOAD_THREAD_COUNT {
        print!("\x1b[2K\r");
        if let Some(t) = active.get(i) {
            let status = match &t.status {
                TrackStatus::Searching => {
                    let secs = t.search_start.map(|s| s.elapsed().as_secs()).unwrap_or(0);
                    format!("Searching... ({secs}s)")
                }
                TrackStatus::Downloading(p) => format!("Downloading {}", format_bar(*p)),
                TrackStatus::WritingMetadata => "Writing metadata...".to_string(),
                _ => unreachable!(),
            };
            println!("{:<40}  {status}", truncate(&t.label, 40));
        } else {
            println!();
        }
    }
    print!("\x1b[2K\r");
    println!("Done: {done}/{}  Failed: {failed}", tracks.len());

    *lines_drawn = DOWNLOAD_THREAD_COUNT + 1;
    io::stdout().flush().ok();
}

fn print_plain(state: &TrackState) {
    match &state.status {
        TrackStatus::Searching => println!("Searching: {}", state.label),
        TrackStatus::Downloading(p) if *p >= 99.9 => println!("Downloaded: {}", state.label),
        TrackStatus::WritingMetadata => println!("Writing metadata: {}", state.label),
        TrackStatus::Done => println!("Done: {}", state.label),
        TrackStatus::Failed => eprintln!("FAILED: {}", state.label),
        _ => {}
    }
}
