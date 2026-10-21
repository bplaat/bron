/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::process::exit;

use crate::args::Args;
use crate::manifest::Manifest;

mod app;
mod appex;
mod ios;
mod support;

pub(crate) fn bundle(args: &Args, target_dir: &str, manifest: &Manifest) {
    if args.ios {
        if args.zip || args.dmg {
            eprintln!("--zip and --dmg are not supported for iOS bundles");
            exit(1);
        }
        ios::bundle(args, target_dir, manifest);
        return;
    }
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
    if args.run {
        app::run(target_dir, bundle_metadata);
    }
}
