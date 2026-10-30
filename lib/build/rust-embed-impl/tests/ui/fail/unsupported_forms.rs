/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use rust_embed_impl::RustEmbed;

#[derive(RustEmbed)]
#[folder = "tests/ui/assets"]
enum Assets {}

#[derive(RustEmbed)]
#[folder = "tests/ui/assets"]
struct Generic<const N: usize>;

fn main() {}
