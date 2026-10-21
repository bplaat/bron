/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A simple small-http server GET variable example

use std::net::{Ipv4Addr, TcpListener};

use serde::Deserialize;
use small_http::{Request, Response, Status};

#[derive(Deserialize)]
struct GreetBody {
    name: String,
}

fn handler(req: Request) -> Response {
    let path = req.url.path();
    println!("{} {}", req.method, path);

    if path == "/" {
        let name = if let Some(query) = req.url.query() {
            match serde_urlencoded::from_str::<GreetBody>(query) {
                Ok(body) => body.name,
                Err(_) => {
                    return Response::with_status(Status::BadRequest)
                        .header("Content-Type", "text/html")
                        .body("<h1>400 Bad Request</h1>");
                }
            }
        } else {
            "World".to_string()
        };
        return Response::with_header("Content-Type", "text/html")
            .body(format!("<h1>Hello {name}!</h1>"));
    }

    Response::with_status(Status::NotFound)
        .header("Content-Type", "text/html")
        .body("<h1>404 Not Found</h1>")
}

fn main() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 8080))
        .unwrap_or_else(|_| panic!("Can't bind to port"));
    small_http::serve(listener, handler);
}
