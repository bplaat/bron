/*
 * Copyright (c) 2024-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A simple HTTP server example

use std::net::{Ipv4Addr, TcpListener};
use std::sync::LazyLock;
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use simple_useragent::UserAgentParser;
use small_http::{Method, Request, Response, Status};

const HTTP_PORT: u16 = 8080;

static USER_AGENT_PARSER: LazyLock<UserAgentParser> = LazyLock::new(UserAgentParser::new);

#[derive(Deserialize)]
struct GreetBody {
    name: String,
}

fn handler(req: Request) -> Response {
    let path = req.url.path();
    println!("{} {}", req.method, path);

    if path == "/" {
        return Response::with_header("Content-Type", "text/html").body("<h1>Hello World!</h1>");
    }

    if path == "/greet" {
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

    if path == "/redirect" {
        return Response::with_redirect("/");
    }

    if path == "/sleep" {
        thread::sleep(Duration::from_secs(5));
        return Response::with_header("Content-Type", "text/html").body("<h1>Sleeping done!</h1>");
    }

    if path == "/ipinfo" {
        let data_res = match Request::get("https://ipinfo.io/json").fetch() {
            Ok(res) => res,
            Err(_) => {
                return Response::with_status(Status::InternalServerError)
                    .header("Content-Type", "text/html")
                    .body("<h1>Can't fetch ipinfo.io</h1>");
            }
        };
        return Response::with_header("Content-Type", "application/json").body(data_res.body);
    }

    if path == "/useragent" {
        if let Some(user_agent) = req.headers.get("User-Agent") {
            return Response::with_json(USER_AGENT_PARSER.parse(user_agent));
        }
        return Response::with_status(Status::BadRequest).body("<h1>Can't parse user agent</h1>");
    }

    Response::with_status(Status::NotFound)
        .header("Content-Type", "text/html")
        .body("<h1>404 Not Found</h1>")
}

fn main() {
    println!("Server is listening on: http://localhost:{HTTP_PORT}/");
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, HTTP_PORT))
        .unwrap_or_else(|_| panic!("Can't bind to port: {HTTP_PORT}"));
    small_http::serve(listener, handler);
}
