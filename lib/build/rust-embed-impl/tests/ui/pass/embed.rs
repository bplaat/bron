/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use rust_embed::{Embed, RustEmbed};

#[derive(Embed)]
#[folder = "../../../../lib/build/rust-embed-impl/tests/ui/assets"]
struct Assets;

fn main() {
    let _ = Assets::get("hello.txt");
    let _ = Assets::iter();
}
