/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![no_std]

extern crate std;

use rust_embed::{Embed, RustEmbed};

#[derive(Embed)]
#[folder = "../../../../lib/build/rust-embed-impl/tests/ui/assets"]
struct Assets;

fn get<E: RustEmbed>(file_path: &str) -> Option<rust_embed::EmbeddedFile> {
    E::get(file_path)
}

fn main() {
    assert!(Assets::get("hello.txt").is_some());
    assert_eq!(&*Assets::get("a-b.txt").unwrap().data, b"dash\n");
    assert_eq!(&*get::<Assets>("a_b.txt").unwrap().data, b"underscore\n");
    assert!(Assets::get("missing.txt").is_none());
    assert_eq!(Assets::iter().count(), 3);
}
