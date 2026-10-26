/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../../README.md")]

use std::collections::{HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, FileTimes, OpenOptions};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use std::{env, io};

use zip::{ZipArchive, ZipFile};

fn ensure_directory(root: &Path, relative: &Path) -> Result<(), String> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(format!("unsafe archive path: {}", relative.display()));
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(format!("{} is not a directory", path.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
        }
    }
    Ok(())
}

fn apply_metadata(file: &File, mode: Option<u32>, modified: Option<SystemTime>) -> io::Result<()> {
    if let Some(modified) = modified {
        file.set_times(FileTimes::new().set_modified(modified))?;
    }
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        // Ownership and special permission bits are intentionally not restored.
        file.set_permissions(fs::Permissions::from_mode(mode & 0o777))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

struct PendingLink {
    relative: PathBuf,
    target: PathBuf,
    #[cfg(unix)]
    modified: Option<SystemTime>,
}

fn target_components(target: &Path) -> Result<VecDeque<OsString>, String> {
    if target.as_os_str().is_empty() {
        return Err("empty symlink target".into());
    }
    target
        .components()
        .map(|component| match component {
            Component::Normal(name) => Ok(name.to_os_string()),
            Component::ParentDir => Ok(OsString::from("..")),
            Component::CurDir => Ok(OsString::from(".")),
            _ => Err(format!("absolute symlink target: {}", target.display())),
        })
        .collect()
}

fn validate_link(
    root: &Path,
    link: &PendingLink,
    links: &HashMap<PathBuf, PathBuf>,
) -> Result<(), String> {
    let mut path = link
        .relative
        .parent()
        .unwrap_or(Path::new(""))
        .to_path_buf();
    let mut components = target_components(&link.target)?;
    let mut followed = 0;
    while let Some(component) = components.pop_front() {
        if component == OsStr::new(".") {
            continue;
        }
        if component == OsStr::new("..") {
            if !path.pop() {
                return Err(format!(
                    "symlink target escapes destination: {}",
                    link.relative.display()
                ));
            }
            continue;
        }
        path.push(component);
        let target = if let Some(target) = links.get(&path) {
            Some(target.clone())
        } else {
            match fs::symlink_metadata(root.join(&path)) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    Some(fs::read_link(root.join(&path)).map_err(|error| error.to_string())?)
                }
                Ok(_) => None,
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.to_string()),
            }
        };
        if let Some(target) = target {
            followed += 1;
            if followed > 40 {
                return Err("symlink cycle or excessive link depth".into());
            }
            path.pop();
            let mut target = target_components(&target)?;
            target.append(&mut components);
            components = target;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_link(root: &Path, link: &PendingLink) -> io::Result<()> {
    let path = root.join(&link.relative);
    std::os::unix::fs::symlink(&link.target, &path)?;
    // Set the time of the link itself, never of its target.
    if let Some(modified) = link.modified
        && let Err(error) = fs::set_times_nofollow(&path, FileTimes::new().set_modified(modified))
    {
        _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_link(_root: &Path, _link: &PendingLink) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symlink extraction is supported only on Unix",
    ))
}

fn link_target(entry: &mut ZipFile) -> Result<PathBuf, String> {
    let mut bytes = Vec::new();
    io::Read::read_to_end(entry, &mut bytes).map_err(|error| error.to_string())?;
    if bytes.contains(&0) {
        return Err("NUL-containing symlink target".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes)
            .map(PathBuf::from)
            .map_err(|error| error.to_string())
    }
}

fn run() -> Result<(), String> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let (list, archive_path, destination) = match args.as_slice() {
        [flag, archive] if flag == OsStr::new("-l") => (true, archive, None),
        [archive] => (false, archive, None),
        [archive, flag, directory] if flag == OsStr::new("-d") => {
            (false, archive, Some(Path::new(directory)))
        }
        _ => return Err("usage: unzip [-l] archive [-d directory]".into()),
    };
    let archive_file = File::open(archive_path)
        .map_err(|error| format!("{}: {error}", Path::new(archive_path).display()))?;
    let mut archive = ZipArchive::new(archive_file).map_err(|error| error.to_string())?;
    let root = if list {
        None
    } else {
        let directory = destination.unwrap_or(Path::new("."));
        fs::create_dir_all(directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        Some(fs::canonicalize(directory).map_err(|error| error.to_string())?)
    };
    let mut directories = HashMap::new();
    let mut links = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().to_owned();
        let relative = Path::new(&name);
        if name.contains(['\\', ':'])
            || relative.components().next().is_none()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!("unsafe archive path: {name}"));
        }
        if let Some(root) = &root {
            let path = root.join(relative);
            if entry.is_dir() {
                ensure_directory(root, relative)?;
                directories.insert(
                    relative.to_path_buf(),
                    (entry.unix_mode(), entry.last_modified()),
                );
            } else {
                if let Some(parent) = relative.parent() {
                    ensure_directory(root, parent)?;
                }
                if entry.is_symlink() {
                    links.push(PendingLink {
                        relative: relative.to_path_buf(),
                        target: link_target(&mut entry)?,
                        #[cfg(unix)]
                        modified: entry.last_modified(),
                    });
                } else if entry.is_file() {
                    let mut output = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .map_err(|error| format!("{}: {error}", path.display()))?;
                    let result = io::copy(&mut entry, &mut output).and_then(|_| {
                        apply_metadata(&output, entry.unix_mode(), entry.last_modified())
                    });
                    if let Err(error) = result {
                        drop(output);
                        _ = fs::remove_file(&path);
                        return Err(format!("{}: {error}", path.display()));
                    }
                } else {
                    return Err(format!("unsupported special file: {name}"));
                }
            }
        }
        println!("{name}");
    }
    if let Some(root) = &root {
        let targets: HashMap<PathBuf, PathBuf> = links
            .iter()
            .map(|link| (link.relative.clone(), link.target.clone()))
            .collect();
        if links.len() != targets.len() {
            return Err("duplicate symlink entries".into());
        }
        for link in &links {
            if fs::symlink_metadata(root.join(&link.relative)).is_ok() {
                return Err(format!("{} already exists", link.relative.display()));
            }
        }
        for link in &links {
            validate_link(root, link, &targets)?;
        }
        // Links are created after regular entries, so extraction never writes through them.
        let mut created = Vec::new();
        let result = (|| {
            for link in &links {
                if let Some(parent) = link.relative.parent() {
                    ensure_directory(root, parent)?;
                }
                create_link(root, link)
                    .map_err(|error| format!("{}: {error}", link.relative.display()))?;
                created.push(root.join(&link.relative));
            }
            // Resolve actual links as well, including aliases on case-insensitive filesystems.
            let actual = HashMap::new();
            for link in &links {
                validate_link(root, link, &actual)?;
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            for path in created {
                _ = fs::remove_file(path);
            }
            return Err(error);
        }
        let mut directories = directories
            .into_iter()
            .map(|(path, (mode, time))| (path, mode, time))
            .collect::<Vec<_>>();
        directories.sort_by_key(|(path, _, _)| std::cmp::Reverse(path.components().count()));
        for (path, mode, modified) in directories {
            ensure_directory(root, &path)?;
            let path = root.join(path);
            #[cfg(windows)]
            let file = {
                use std::os::windows::fs::OpenOptionsExt;
                OpenOptions::new()
                    .read(true)
                    .access_mode(0x100)
                    .custom_flags(0x02000000)
                    .open(&path)
            };
            #[cfg(not(windows))]
            let file = File::open(&path);
            let file = file.map_err(|error| error.to_string())?;
            apply_metadata(&file, mode, modified).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("unzip: {error}");
        std::process::exit(1);
    }
}
