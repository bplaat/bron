/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A simple small-http server example

use std::net::{Ipv4Addr, TcpListener};

use small_http::{Request, Response};

fn handler(_req: Request) -> Response {
    Response::with_body("Hello World!")
}

fn main() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 8080))
        .unwrap_or_else(|_| panic!("Can't bind to port"));
    small_http::serve(listener, handler);
}
