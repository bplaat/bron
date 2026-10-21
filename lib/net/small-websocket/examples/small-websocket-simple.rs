/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A simple small-http server websocket upgrade example

use std::net::{Ipv4Addr, TcpListener};

use small_http::{Request, Response, Status};
use small_websocket::Message;

fn handler(req: Request) -> Response {
    let path = req.url.path();
    println!("{} {}", req.method, path);

    if path == "/ws" {
        return small_websocket::upgrade(&req, |mut ws| {
            println!(
                "Client connected: {}",
                ws.peer_addr().expect("Can't get client addr")
            );
            loop {
                let message = ws.recv().expect("Failed to receive message");
                match message {
                    Message::Text(text) => {
                        println!("Client recv: {text}");
                        ws.send(Message::Text(text))
                            .expect("Failed to send message");
                    }
                    Message::Close(_, _) => break,
                    _ => {}
                }
            }
            println!("Client disconnected");
        });
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
