/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::args::Args;
use crate::manifest::Manifest;

mod app;
mod appex;
mod support;

pub(crate) fn bundle(args: &Args, target_dir: &str, manifest: &Manifest) {
    let bundle_metadata = &manifest.package.metadata.bundle;
    app::build(&args.path, target_dir, manifest);
    appex::build_all(&args.path, target_dir, manifest);
    app::sign(&args.path, target_dir, bundle_metadata);

    if args.zip {
        app::create_zip(target_dir, bundle_metadata);
    }
    if args.dmg {
        app::create_dmg(target_dir, bundle_metadata);
    }
}
