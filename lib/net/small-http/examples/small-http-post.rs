/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A simple small-http server GET variable example

use std::net::{Ipv4Addr, TcpListener};

use serde::Deserialize;
use small_http::{Method, Request, Response, Status};

#[derive(Deserialize)]
struct GreetBody {
    name: String,
}

fn handler(req: Request) -> Response {
    let path = req.url.path();
    println!("{} {}", req.method, path);

    if path == "/" {
        if req.method != Method::Post {
            return Response::with_status(Status::MethodNotAllowed)
                .header("Content-Type", "text/html")
                .body("<h1>405 Method Not Allowed</h1>");
        }

        let body =
            match serde_urlencoded::from_bytes::<GreetBody>(req.body.as_deref().unwrap_or(&[])) {
                Ok(body) => body,
                Err(_) => {
                    return Response::with_status(Status::BadRequest)
                        .header("Content-Type", "text/html")
                        .body("<h1>400 Bad Request</h1>");
                }
            };
        return Response::with_header("Content-Type", "text/html")
            .body(format!("<h1>Hello {}!</h1>", body.name));
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
