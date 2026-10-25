/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub(crate) fn normalize_path(path: &Path, root: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}

fn is_directory_without_following_symlinks(path: &Path) -> Result<bool> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {}", path.display()))?;
    Ok(metadata.is_dir() && !metadata.file_type().is_symlink())
}

pub(crate) fn collect_source_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let path = entry?.path();
        if is_directory_without_following_symlinks(&path)? {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if matches!(
                name,
                ".git"
                    | "node_modules"
                    | "dist"
                    | "src-gen"
                    | "target"
                    | "playwright-report"
                    | "test-results"
            ) {
                continue;
            }
            collect_source_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

pub(crate) fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let path = entry?.path();
        if is_directory_without_following_symlinks(&path)? {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

pub(crate) fn collect_paths(
    directory: &Path,
    paths: &mut Vec<PathBuf>,
    matches: &impl Fn(&Path, bool) -> bool,
) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let path = entry?.path();
        let is_dir = is_directory_without_following_symlinks(&path)?;
        if is_dir && path.file_name().and_then(|name| name.to_str()) == Some(".git") {
            continue;
        }
        if matches(&path, is_dir) {
            paths.push(path);
        } else if is_dir {
            collect_paths(&path, paths, matches)?;
        }
    }
    Ok(())
}

pub(crate) fn relative_slash(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn remove_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))
    } else if metadata.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))
    } else {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))
    }
}

pub(crate) fn remove_path_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        remove_path(path)
    } else {
        Ok(())
    }
}

pub(crate) fn remove_directory_except(directory: &Path, preserve: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to read {}", directory.display()))?
    {
        let path = entry?.path();
        if path == preserve {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            remove_directory_except(&path, preserve)?;
            if !preserve.starts_with(&path) {
                fs::remove_dir(&path)
                    .with_context(|| format!("failed to remove {}", path.display()))?;
            }
        } else {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn copy_directory_contents(source: &Path, destination: &Path) -> Result<()> {
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let path = entry?.path();
        let target = destination.join(path.file_name().context("source entry has no file name")?);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            copy_symlink(&path, &target)?;
        } else if metadata.is_dir() {
            copy_directory(&path, &target)?;
        } else {
            fs::copy(&path, &target).with_context(|| {
                format!("failed to copy {} to {}", path.display(), target.display())
            })?;
        }
    }
    Ok(())
}

pub(crate) fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    copy_directory_contents(source, destination)
}

fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target = fs::read_link(source)?;
    cfg_select! {
        unix => std::os::unix::fs::symlink(target, destination),
        windows => {
            if source.is_dir() {
                std::os::windows::fs::symlink_dir(target, destination)
            } else {
                std::os::windows::fs::symlink_file(target, destination)
            }
        }
        _ => compile_error!("Unsupported platform"),
    }
    .with_context(|| format!("failed to copy symlink {}", source.display()))
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::env;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn normalize_path_keeps_absolute_and_joins_relative() {
        let root = env::temp_dir().join("workspace");
        let absolute = root.join("lib/foo/Cargo.toml");
        assert_eq!(normalize_path(&absolute, &root), absolute);
        assert_eq!(
            normalize_path(Path::new("lib/foo/Cargo.toml"), &root),
            root.join("lib/foo/Cargo.toml")
        );
    }

    #[test]
    fn relative_slash_strips_root_and_uses_forward_slashes() {
        let root = env::temp_dir();
        assert_eq!(
            relative_slash(&root, &root.join("lib").join("foo.rs")),
            "lib/foo.rs"
        );
    }

    #[test]
    fn relative_slash_returns_unrelated_path_unchanged() {
        let base = scratch("relslash");
        let outside = base.join("outside").join("x.rs");
        assert_eq!(
            relative_slash(&base.join("root"), &outside),
            outside.to_string_lossy().replace('\\', "/")
        );
    }

    #[test]
    fn collect_source_files_skips_generated_directories() -> Result<()> {
        let root = scratch("collect-src");
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("target/debug"))?;
        fs::create_dir_all(root.join("node_modules/pkg"))?;
        fs::write(root.join("src/main.rs"), "")?;
        fs::write(root.join("target/debug/app"), "")?;
        fs::write(root.join("node_modules/pkg/index.js"), "")?;
        fs::write(root.join("README.md"), "")?;

        let mut files = Vec::new();
        collect_source_files(&root, &mut files)?;
        let relative = files
            .iter()
            .map(|file| relative_slash(&root, file))
            .collect::<BTreeSet<_>>();

        assert!(relative.contains("src/main.rs"));
        assert!(relative.contains("README.md"));
        assert!(!relative.iter().any(|path| path.starts_with("target/")));
        assert!(
            !relative
                .iter()
                .any(|path| path.starts_with("node_modules/"))
        );

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn collect_paths_matches_predicate_and_stops_descending() -> Result<()> {
        let root = scratch("collect-paths");
        fs::create_dir_all(root.join("keep/inner"))?;
        fs::create_dir_all(root.join("src"))?;
        fs::write(root.join("keep/inner/deep.txt"), "")?;
        fs::write(root.join("src/data.db"), "")?;
        fs::write(root.join("src/main.rs"), "")?;

        let mut paths = Vec::new();
        collect_paths(&root, &mut paths, &|path, is_dir| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if is_dir {
                name == "keep"
            } else {
                name.contains(".db")
            }
        })?;
        let relative = paths
            .iter()
            .map(|path| relative_slash(&root, path))
            .collect::<BTreeSet<_>>();

        assert!(relative.contains("keep"));
        assert!(relative.contains("src/data.db"));
        // `keep` matched as a directory, so its children are not visited.
        assert!(!relative.iter().any(|path| path.starts_with("keep/")));
        assert!(!relative.contains("src/main.rs"));

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn recursive_collectors_do_not_follow_directory_symlinks() -> Result<()> {
        use std::os::unix::fs::symlink;

        let base = scratch("symlink-walkers");
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(outside.join("target"))?;
        fs::write(outside.join("external.rs"), "outside")?;
        fs::write(outside.join("target/external.db"), "outside")?;
        symlink(&outside, root.join("external"))?;
        symlink(&root, root.join("cycle"))?;

        let mut source_files = Vec::new();
        collect_source_files(&root, &mut source_files)?;
        assert_eq!(
            source_files
                .iter()
                .map(|path| relative_slash(&root, path))
                .collect::<BTreeSet<_>>(),
            ["cycle".to_owned(), "external".to_owned()]
                .into_iter()
                .collect()
        );

        let mut files = Vec::new();
        collect_files(&root, &mut files)?;
        assert_eq!(
            files
                .iter()
                .map(|path| relative_slash(&root, path))
                .collect::<BTreeSet<_>>(),
            ["cycle".to_owned(), "external".to_owned()]
                .into_iter()
                .collect()
        );

        let mut generated = Vec::new();
        collect_paths(&root, &mut generated, &|path, is_dir| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            (is_dir && name == "target") || (!is_dir && name.contains(".db"))
        })?;
        assert!(generated.is_empty());
        assert!(outside.join("target/external.db").exists());

        fs::remove_dir_all(&base)?;
        Ok(())
    }

    #[test]
    fn copy_directory_recreates_tree() -> Result<()> {
        let base = scratch("copy-dir");
        let source = base.join("src");
        let destination = base.join("dst");
        fs::create_dir_all(source.join("nested"))?;
        fs::write(source.join("a.txt"), "alpha")?;
        fs::write(source.join("nested/b.txt"), "beta")?;

        copy_directory(&source, &destination)?;

        assert_eq!(fs::read_to_string(destination.join("a.txt"))?, "alpha");
        assert_eq!(
            fs::read_to_string(destination.join("nested/b.txt"))?,
            "beta"
        );

        fs::remove_dir_all(&base)?;
        Ok(())
    }

    #[test]
    fn remove_path_deletes_files_and_directories() -> Result<()> {
        let base = scratch("remove-path");
        fs::create_dir_all(base.join("dir"))?;
        fs::write(base.join("dir/file.txt"), "")?;
        fs::write(base.join("loose.txt"), "")?;

        remove_path(&base.join("loose.txt"))?;
        remove_path(&base.join("dir"))?;
        assert!(!base.join("loose.txt").exists());
        assert!(!base.join("dir").exists());

        // Missing paths are a no-op rather than an error.
        remove_path_if_exists(&base.join("missing"))?;

        fs::remove_dir_all(&base)?;
        Ok(())
    }

    #[test]
    fn directory_cleanup_preserves_running_executable_path() -> Result<()> {
        let root = env::temp_dir().join(format!("xtask-clean-test-{}", std::process::id()));
        let executable = root.join("debug/xtask.exe");
        fs::create_dir_all(
            executable
                .parent()
                .context("test executable has no parent")?,
        )?;
        fs::create_dir_all(root.join("release"))?;
        fs::write(&executable, "running")?;
        fs::write(root.join("debug/unrelated.pdb"), "generated")?;
        fs::write(root.join("release/app.exe"), "generated")?;

        remove_directory_except(&root, &executable)?;

        assert!(executable.exists());
        assert!(!root.join("debug/unrelated.pdb").exists());
        assert!(!root.join("release").exists());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
