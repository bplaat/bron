/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::path::{Path, PathBuf};

/// Android SDK, NDK, JDK discovery and Java-to-dex compilation.
pub mod android;
/// macOS executable metadata embedding.
pub mod macos;
mod npm;
/// Windows application manifest and resource compilation.
pub mod windows;

pub use npm::npm_build;

/// Finds a shared library by name in the supplied directory order.
pub fn find_library(search_dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    let prefix = format!("lib{name}.so");
    for directory in search_dirs {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        let mut candidates = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|file| file.to_str())
                    .is_some_and(|file| file == prefix || file.starts_with(&format!("{prefix}.")))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|path| path.file_name().is_some_and(|file| file == prefix.as_str()));
        if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
            return Some(path);
        }
    }
    None
}

/// Finds a shared library or panics with the searched directories.
pub fn require_library(search_dirs: &[PathBuf], name: &str) -> PathBuf {
    find_library(search_dirs, name).unwrap_or_else(|| {
        panic!(
            "could not find the {name} runtime library; searched: {}",
            display_paths(search_dirs)
        )
    })
}

/// Emits Cargo linker directives for an exact shared library file.
pub fn link_library(path: &Path) {
    let directory = path
        .parent()
        .expect("library should have a parent directory");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("library filename should be valid UTF-8");
    println!("cargo::rustc-link-search=native={}", directory.display());
    println!("cargo::rustc-link-lib=dylib:+verbatim={file_name}");
}

/// Checks whether a library file contains a symbol name.
pub fn library_has_symbol(path: &Path, symbol: &[u8]) -> bool {
    std::fs::read(path)
        .is_ok_and(|bytes| bytes.windows(symbol.len()).any(|window| window == symbol))
}

/// Returns standard output when a command starts and exits successfully.
pub fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy_owned(output.stdout))
}

/// Appends a nonempty path unless it is already present.
pub fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.as_os_str().is_empty() && !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

/// Formats search directories for diagnostics.
pub fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Returns standard Unix library directories, including the compiler multiarch paths.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut search_dirs = Vec::new();
    if let Some(multiarch) = command_stdout("cc", &["-print-multiarch"]) {
        let multiarch = multiarch.trim();
        if !multiarch.is_empty() {
            push_unique(&mut search_dirs, PathBuf::from("/usr/lib").join(multiarch));
            push_unique(&mut search_dirs, PathBuf::from("/lib").join(multiarch));
        }
    }
    for path in [
        "/usr/local/lib",
        "/usr/pkg/lib",
        "/usr/lib64",
        "/lib64",
        "/usr/lib",
        "/lib",
    ] {
        push_unique(&mut search_dirs, PathBuf::from(path));
    }
    search_dirs
}

/// Links each named shared library from the supplied search directories.
pub fn link_libraries(search_dirs: &[PathBuf], names: &[&str]) {
    for name in names {
        link_library(&require_library(search_dirs, name));
    }
}

/// Requires a marker symbol in a library, using the supplied diagnostic on failure.
pub fn require_symbol(library: &Path, symbol: &[u8], message: &str) {
    assert!(library_has_symbol(library, symbol), "{message}");
}

/// Emits a Cargo cfg flag when a named library contains a marker symbol.
pub fn cfg_if_symbol(search_dirs: &[PathBuf], library: &str, symbol: &[u8], cfg: &str) {
    if let Some(path) = find_library(search_dirs, library)
        && library_has_symbol(&path, symbol)
    {
        println!("cargo::rustc-cfg={cfg}");
    }
}

/// Finds libraries together in one directory with the same filename suffix.
///
/// Names omit `lib` and the suffix. Directories and suffixes are searched in the
/// supplied order. Returns the matching directory and suffix.
pub fn find_library_group(
    search_dirs: &[PathBuf],
    names: &[&str],
    suffixes: &[&str],
) -> Option<(PathBuf, String)> {
    for directory in search_dirs {
        for suffix in suffixes {
            if names
                .iter()
                .all(|name| directory.join(format!("lib{name}{suffix}")).is_file())
            {
                return Some((directory.clone(), (*suffix).to_owned()));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{find_library, find_library_group, library_has_symbol};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "bron-build-utils-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create fixture directory");
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove fixture directory");
        }
    }

    #[test]
    fn discovers_runtime_libraries_in_search_order_and_checks_symbols() {
        let fixture = Fixture::new();
        let first = fixture.0.join("first");
        let second = fixture.0.join("second");
        fs::create_dir(&first).expect("create first search directory");
        fs::create_dir(&second).expect("create second search directory");
        let directories = [fixture.0.join("missing"), first.clone(), second.clone()];
        fs::write(first.join("libexample.something"), []).expect("write unrelated file");
        fs::create_dir(first.join("libexample.so.9")).expect("create misleading directory");
        assert_eq!(find_library(&directories, "example"), None);

        let fallback = second.join("libexample.so");
        fs::write(&fallback, []).expect("write fallback library");
        assert_eq!(find_library(&directories, "example"), Some(fallback));

        let runtime = first.join("libexample.so.1");
        fs::write(&runtime, b"prefix\0example_feature\0suffix").expect("write runtime library");
        fs::write(first.join("libexample.so"), []).expect("write unversioned library");
        assert_eq!(find_library(&directories, "example"), Some(runtime.clone()));
        assert!(library_has_symbol(&runtime, b"example_feature"));
        assert!(!library_has_symbol(&runtime, b"missing_feature"));
        assert!(!library_has_symbol(
            &first.join("missing.so"),
            b"example_feature"
        ));
    }

    #[test]
    fn library_groups_require_matching_directories_and_suffixes() {
        let fixture = Fixture::new();
        let first = fixture.0.join("first");
        let second = fixture.0.join("second");
        fs::create_dir(&first).expect("create first search directory");
        fs::create_dir(&second).expect("create second search directory");
        let directories = [first.clone(), second.clone()];
        let names = ["one", "two"];
        let suffixes = [".so.4", ".so.3"];
        fs::write(first.join("libone.so.4"), []).expect("write first library");
        fs::write(second.join("libtwo.so.4"), []).expect("write second library");
        fs::write(first.join("libtwo.so.3"), []).expect("write mismatched version");
        assert_eq!(find_library_group(&directories, &names, &suffixes), None);
        fs::write(first.join("libone.so.3"), []).expect("write matching version");
        fs::write(second.join("libone.so.4"), []).expect("complete second group");
        assert_eq!(
            find_library_group(&directories, &names, &suffixes),
            Some((first.clone(), ".so.3".to_owned()))
        );
        fs::write(first.join("libtwo.so.4"), []).expect("complete preferred version");
        assert_eq!(
            find_library_group(&directories, &names, &suffixes),
            Some((first, ".so.4".to_owned()))
        );
    }
}
