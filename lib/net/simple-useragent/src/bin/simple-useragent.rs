/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../../README.md")]

use std::process::ExitCode;

use simple_useragent::UserAgentParser;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_none() {
        eprintln!("Usage: simple-useragent <user-agent> [user-agent ...]");
        return ExitCode::FAILURE;
    }

    let parser = UserAgentParser::new();
    for (index, input) in args.enumerate() {
        if index > 0 {
            println!();
        }
        let result = parser.parse(&input);
        println!("User agent: {input}");
        println!("Client: {}", result.client.family);
        println!(
            "Client version: {}",
            result.client.version.as_deref().unwrap_or("unknown")
        );
        println!("OS: {}", result.os.family);
        println!(
            "OS version: {}",
            result.os.version.as_deref().unwrap_or("unknown")
        );
    }
    ExitCode::SUCCESS
}
