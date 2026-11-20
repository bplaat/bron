/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Serves the website: the static files Vite builds into web/dist and the LAN relay between the
//! visitors' Windows XP desktops on /relay. Environment: PORT (8080), DIST_PATH (web/dist), ORIGIN
//! and TRUST_PROXY, see relay.rs.

use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use std::{env, fs};

use jiff::Timestamp;
use jiff::fmt::rfc2822::DateTimePrinter;
use jiff::tz::TimeZone;
use small_http::{Method, Request, Response, Status};

use crate::relay::{Config, Relay};

mod relay;

fn main() {
    let port = env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(8080);
    let dist = env::var_os("DIST_PATH").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist"),
        PathBuf::from,
    );
    let relay = Relay::new(Config::from_env());

    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .unwrap_or_else(|err| panic!("Can't bind to port {port}: {err}"));
    println!("Serving {} on http://localhost:{port}", dist.display());
    small_http::serve(listener, move |request| {
        if request.url.path() == "/relay" {
            relay.handler(request)
        } else {
            static_file(&dist, request)
        }
    });
}

// Vite's hashed assets never change. The home page is rendered on each request, and the remaining
// files are checked with their Last-Modified date. The disk chunks and snapshot are zstd already.
fn static_file(dist: &Path, request: &Request) -> Response {
    if request.method != Method::Get {
        return Response::with_status(Status::MethodNotAllowed);
    }
    let mut relative = request.url.path().trim_start_matches('/').to_string();
    if relative.is_empty() || relative.ends_with('/') {
        relative.push_str("index.html");
    }
    if relative
        .split('/')
        .any(|part| part.is_empty() || part.starts_with('.') || part.contains('\\'))
    {
        return Response::with_status(Status::NotFound);
    }
    let path = dist.join(&relative);
    let Some(modified) = fs::metadata(&path)
        .ok()
        .filter(|metadata| metadata.is_file())
        .and_then(|metadata| metadata.modified().ok())
    else {
        return Response::with_status(Status::NotFound);
    };
    if relative == "index.html" {
        let Ok(html) = fs::read_to_string(&path) else {
            return Response::with_status(Status::InternalServerError);
        };
        let today = Timestamp::now().to_zoned_datetime(TimeZone::UTC).date();
        let age = today.year() - 2002 - i16::from(today.month() < 7);
        return Response::with_header("Cache-Control", "no-store")
            .header("Content-Type", "text/html; charset=utf-8")
            .body(html.replace("%AGE%", &age.to_string()));
    }
    let last_modified = modified
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| Timestamp::from_second(duration.as_secs() as i64).ok())
        .and_then(|timestamp| {
            DateTimePrinter::new()
                .timestamp_to_rfc9110_string(&timestamp)
                .ok()
        })
        .unwrap_or_default();
    let cache_control = if relative.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    let mut response = Response::with_header("Cache-Control", cache_control)
        .header("Last-Modified", &last_modified);
    if request.headers.get("If-Modified-Since") == Some(last_modified.as_str()) {
        response.status = Status::NotModified;
        return response;
    }
    match fs::read(&path) {
        Ok(body) => response
            .header(
                "Content-Type",
                mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .to_string(),
            )
            .body(body),
        Err(_) => Response::with_status(Status::InternalServerError),
    }
}
