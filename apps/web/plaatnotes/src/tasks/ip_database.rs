/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::context::Context;

fn try_download_mmdb(mmdb_path: &str) {
    let date = chrono::Utc::now().naive_utc().date();
    let (year, month) = (date.year(), date.month());
    let (prev_year, prev_month) = if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    };
    for (y, m) in [(year, month), (prev_year, prev_month)] {
        let url = format!("https://download.db-ip.com/free/dbip-city-lite-{y}-{m:02}.mmdb.gz");
        log::info!("Downloading DB-IP city lite database from {url}...");
        match small_http::Request::get(&url).fetch() {
            Ok(res) if res.status == small_http::Status::Ok => {
                // Free the compressed download before writing the decompressed database
                let decompressed = gzip::decompress(&res.body);
                drop(res);
                let Some(decompressed) = decompressed else {
                    log::warn!("Failed to decompress DB-IP database");
                    continue;
                };
                // Rename a complete file into place, so a mapped database is never truncated
                let partial_path = format!("{mmdb_path}.partial");
                match std::fs::write(&partial_path, &decompressed)
                    .and_then(|()| std::fs::rename(&partial_path, mmdb_path))
                {
                    Ok(()) => {
                        log::info!("DB-IP city lite database saved to {mmdb_path}");
                        return;
                    }
                    Err(e) => log::warn!("Failed to write DB-IP database to {mmdb_path}: {e}"),
                }
            }
            Ok(_) => log::warn!("Unexpected HTTP response when downloading DB-IP database"),
            Err(_) => log::warn!("Failed to connect to download.db-ip.com"),
        }
    }
    log::warn!("Could not download DB-IP city lite database, will fall back to ipinfo.io");
}

pub(crate) fn run(mmdb_path: String, ctx: Context) {
    let is_nonempty = std::fs::metadata(&mmdb_path)
        .map(|m| m.len() > 0)
        .unwrap_or(false);
    if !is_nonempty {
        try_download_mmdb(&mmdb_path);
    }
    let is_nonempty = std::fs::metadata(&mmdb_path)
        .map(|m| m.len() > 0)
        .unwrap_or(false);
    if is_nonempty {
        // Map the database so it is paged in on demand instead of held in memory
        match maxminddb::Reader::open_mmap(&mmdb_path) {
            Ok(reader) => {
                log::info!("Using DB-IP city lite database at {mmdb_path}");
                let _ = ctx.maxminddb_reader.set(reader);
            }
            Err(e) => log::warn!("Failed to open DB-IP database at {mmdb_path}: {e}"),
        }
    }
}
