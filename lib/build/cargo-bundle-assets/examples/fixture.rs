/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Asset storage verification fixture.

#[derive(cargo_bundle_assets::Assets)]
#[folder = "tests/fixtures"]
struct Assets;

fn main() {
    let names: Vec<_> = Assets::iter().collect();
    assert_eq!(names, ["nested/example-file.txt"]);
    let file = Assets::get("nested/example-file.txt").expect("Fixture asset");
    let expected = std::env::args().nth(1).expect("Expected payload argument");
    assert_eq!(file.data.as_ref(), expected.as_bytes());
    assert_eq!(
        matches!(file.data, std::borrow::Cow::Borrowed(_)),
        option_env!("CARGO_BUNDLE").is_none()
    );
    assert!(Assets::get("missing").is_none());
    assert!(
        Assets::try_get("../Cargo.toml")
            .expect("Undeclared name")
            .is_none()
    );
}
