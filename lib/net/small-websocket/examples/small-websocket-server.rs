/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A simple WebSocket echo server

use std::net::{Ipv4Addr, TcpListener};

use small_http::{Request, Response, Status};
use small_websocket::Message;

fn handler(request: Request) -> Response {
    if request.url.path() != "/ws" {
        return Response::with_status(Status::NotFound)
            .header("Content-Type", "text/html")
            .body("<h1>404 Not Found</h1>");
    }

    small_websocket::upgrade(&request, |mut websocket| {
        println!(
            "Client connected: {}",
            websocket.peer_addr().expect("Can't get client address")
        );
        loop {
            match websocket.recv() {
                Ok(Message::Text(text)) => {
                    println!("Received text: {text}");
                    websocket
                        .send(Message::Text(text))
                        .expect("Failed to echo text");
                }
                Ok(Message::Ping(payload)) => {
                    websocket
                        .send(Message::Pong(payload))
                        .expect("Failed to send pong");
                }
                Ok(Message::Close(_, _)) | Err(_) => break,
                Ok(_) => {}
            }
        }
        println!("Client disconnected");
    })
}

fn main() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 8080))
        .unwrap_or_else(|_| panic!("Can't bind to port"));
    small_http::serve(listener, handler);
}
