/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::io::{Cursor, Read};
use std::path::Path;

use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

use crate::context::{Context, DatabaseHelpers};
use crate::models::Note;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeepNote {
    title: Option<String>,
    #[serde(default)]
    text_content: String,
    #[serde(default)]
    is_pinned: bool,
    #[serde(default)]
    is_archived: bool,
    #[serde(default)]
    is_trashed: bool,
    created_timestamp_usec: i64,
    user_edited_timestamp_usec: i64,
}

fn note_from_json(json_bytes: &[u8], user_id: uuid::Uuid) -> Option<Note> {
    let keep_note: KeepNote = serde_json::from_slice(json_bytes).ok()?;
    let title = keep_note.title.as_deref().unwrap_or("").trim().to_string();
    let body = keep_note.text_content.trim().to_string();
    if title.is_empty() && body.is_empty() {
        return None;
    }
    Some(Note {
        user_id,
        title: if title.is_empty() { None } else { Some(title) },
        body,
        is_pinned: keep_note.is_pinned,
        is_archived: keep_note.is_archived,
        is_trashed: keep_note.is_trashed,
        created_at: Timestamp::from_second(keep_note.created_timestamp_usec / 1_000_000)
            .unwrap_or_else(|_| Timestamp::now()),
        updated_at: Timestamp::from_second(keep_note.user_edited_timestamp_usec / 1_000_000)
            .unwrap_or_else(|_| Timestamp::now()),
        ..Default::default()
    })
}

fn insert_notes(notes: Vec<Note>, ctx: &Context) -> Result<usize> {
    let count = notes.len();
    ctx.database.transaction(|database| -> Result<()> {
        for note in notes {
            database.insert_note(note)?;
        }
        Ok(())
    })?;
    Ok(count)
}

/// Import all Google Keep notes from a zip archive in memory.
/// Returns the number of notes successfully imported.
pub(crate) fn import_from_zip_bytes(
    zip_bytes: &[u8],
    ctx: &Context,
    user_id: uuid::Uuid,
) -> Result<usize> {
    let mut notes = Vec::new();
    let mut archive = match zip::ZipArchive::new(Cursor::new(zip_bytes)) {
        Ok(a) => a,
        Err(_) => return Ok(0),
    };
    for i in 0..archive.len() {
        let mut file = match archive.by_index(i) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let is_keep_json = {
            let mut parts = file.name().split('/');
            let mut previous = parts.next();
            parts.any(|part| {
                let is_match = previous == Some("Keep") && part.ends_with(".json");
                previous = Some(part);
                is_match
            })
        };
        if !is_keep_json {
            continue;
        }
        let mut json_bytes = Vec::new();
        if file.read_to_end(&mut json_bytes).is_err() {
            continue;
        }
        if let Some(note) = note_from_json(&json_bytes, user_id) {
            notes.push(note);
        }
    }
    insert_notes(notes, ctx)
}

pub(crate) fn run(path: &str, email: &str, ctx: &Context) -> Result<()> {
    let user_id = ctx
        .database
        .query_some::<uuid::Uuid>("SELECT id FROM users WHERE email = ?", email.to_string())
        .unwrap_or_else(|_| panic!("No user found with email: {email}"));

    let count = if path.ends_with(".zip") {
        let zip_bytes = std::fs::read(path).unwrap_or_else(|_| panic!("Can't read zip: {path}"));
        import_from_zip_bytes(&zip_bytes, ctx, user_id)?
    } else {
        let keep_dir = {
            let keep_sub = Path::new(path).join("Keep");
            if keep_sub.is_dir() {
                keep_sub
            } else {
                Path::new(path).to_path_buf()
            }
        };
        let entries = std::fs::read_dir(&keep_dir)
            .unwrap_or_else(|_| panic!("Can't read directory: {}", keep_dir.display()));
        let mut notes = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Warning: can't read directory entry: {e}");
                    continue;
                }
            };
            let entry_path = entry.path();
            if entry_path.extension().and_then(|e| e.to_str()) == Some("json") {
                let json_bytes = match std::fs::read(&entry_path) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("Warning: can't read {}: {e}", entry_path.display());
                        continue;
                    }
                };
                if let Some(note) = note_from_json(&json_bytes, user_id) {
                    notes.push(note);
                }
            }
        }
        insert_notes(notes, ctx)?
    };

    println!("Imported {count} notes");
    Ok(())
}
