/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Asset storage verification fixture.

use std::borrow::Cow;

#[derive(cargo_bundle_assets::Assets)]
#[folder = "tests/fixtures"]
struct Assets;

#[test]
fn embeds_sorted_names_and_borrows_payloads() {
    assert_eq!(
        Assets::iter().collect::<Vec<_>>(),
        ["nested/example-file.txt"]
    );
    let file = Assets::get("nested/example-file.txt").expect("Embedded fixture");
    assert!(matches!(file.data, Cow::Borrowed(_)));
    assert_eq!(
        file.data.as_ref(),
        include_bytes!("fixtures/nested/example-file.txt")
    );
    assert!(Assets::get("missing").is_none());
    assert!(
        Assets::try_get("../Cargo.toml")
            .expect("Valid fixture")
            .is_none()
    );
}

#[test]
fn implements_existing_rust_embed_trait() {
    fn read<T: cargo_bundle_assets::RustEmbed>() -> Vec<u8> {
        T::get("nested/example-file.txt")
            .expect("Valid fixture")
            .data
            .into_owned()
    }
    assert_eq!(
        read::<Assets>(),
        include_bytes!("fixtures/nested/example-file.txt")
    );
}
