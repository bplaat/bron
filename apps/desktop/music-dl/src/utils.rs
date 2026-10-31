/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::process::exit;

use anyhow::Result;

use crate::args::Args;
use crate::services::metadata::MetadataService;

pub(crate) fn get_album_ids(
    metadata_service: &mut MetadataService,
    args: &Args,
) -> Result<Vec<i64>> {
    Ok(if args.is_artist {
        let artist_id = if args.is_id {
            args.query.parse()?
        } else {
            let artists = metadata_service.seach_artist(&args.query)?;
            if artists.is_empty() {
                eprintln!("No artist found");
                exit(1);
            }
            artists[0].id
        };

        let albums = metadata_service.get_artist_albums(artist_id)?;
        if args.with_singles {
            albums.iter().map(|album| album.id).collect()
        } else {
            albums
                .iter()
                .filter(|album| {
                    (album.r#type == "album" || album.r#type == "ep")
                        && album.record_type != "single"
                })
                .map(|album| album.id)
                .collect()
        }
    } else if args.is_id {
        vec![args.query.parse()?]
    } else {
        let albums = metadata_service.search_album(&args.query)?;
        if albums.is_empty() {
            eprintln!("No album found");
            exit(1);
        }
        vec![albums[0].id]
    })
}

pub(crate) fn escape_path(path: &str) -> String {
    let path = path.replace(
        |c: char| {
            matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control()
        },
        "_",
    );
    // Windows drops trailing dots and spaces, and names of only dots like .. would leave the folder
    match path.trim_end_matches(['.', ' ']) {
        "" => "_".to_string(),
        path => path.to_string(),
    }
}

pub(crate) fn format_bar(percent: f32) -> String {
    const W: usize = 20;
    let filled = ((percent / 100.0) * W as f32).round() as usize;
    let filled = filled.min(W);
    let mut bar = String::with_capacity(W + 2);
    bar.push('[');
    if filled > 0 {
        for _ in 0..filled - 1 {
            bar.push('=');
        }
        bar.push(if filled == W { '=' } else { '>' });
    }
    for _ in filled..W {
        bar.push('-');
    }
    bar.push(']');
    format!("{bar} {percent:>5.1}%")
}

pub(crate) fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_escape_path() {
        assert_eq!(escape_path("AC/DC: Live?"), "AC_DC_ Live_");
        assert_eq!(escape_path("Tab\there"), "Tab_here");
        assert_eq!(escape_path(".."), "_");
        assert_eq!(escape_path(". . "), "_");
        assert_eq!(escape_path("Vol. 1..."), "Vol. 1");
        assert_eq!(escape_path(".hidden"), ".hidden");
    }
}
