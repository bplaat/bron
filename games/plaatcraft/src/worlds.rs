/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The saved worlds, each a SQLite file in the worlds folder named after the world.

use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use plaatcraft_engine::protocol::valid_char;

use crate::database::Database;

const EXTENSION: &str = "db";
// SQLite keeps these next to an open or uncleanly closed database.
const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];
pub(crate) const MAX_NAME_LENGTH: usize = 32;
const DEFAULT_NAME: &str = "New World";

pub(crate) struct World {
    pub(crate) name: String,
    pub(crate) modified: SystemTime,
}

pub(crate) struct Worlds {
    directory: PathBuf,
}

impl Worlds {
    // App sandboxes have no writable working directory, so the worlds live in the data directory.
    pub(crate) fn new() -> io::Result<Self> {
        let data = directories::ProjectDirs::from("nl", "bplaat", "PlaatCraft")
            .ok_or_else(|| io::Error::other("Find data directory"))?
            .data_dir()
            .to_owned();
        Self::with_directory(data.join("worlds"))
    }

    fn with_directory(directory: PathBuf) -> io::Result<Self> {
        std::fs::create_dir_all(&directory)?;
        Ok(Self { directory })
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{name}.{EXTENSION}"))
    }

    /// The worlds, the most recently played first.
    pub(crate) fn list(&self) -> Vec<World> {
        let mut worlds: Vec<_> = std::fs::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension()? != EXTENSION {
                    return None;
                }
                let name = path.file_stem()?.to_str()?.to_owned();
                let modified = entry.metadata().ok()?.modified().ok()?;
                validate(&name).is_ok().then_some(World { name, modified })
            })
            .collect();
        worlds.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.name.cmp(&b.name))
        });
        worlds
    }

    // File systems can ignore case, so names that only differ in case are taken too.
    fn taken(&self, name: &str) -> bool {
        self.path(name).exists()
            || self
                .list()
                .iter()
                .any(|world| world.name.eq_ignore_ascii_case(name))
    }

    /// The first free name for a new world.
    pub(crate) fn unused_name(&self) -> String {
        (1..)
            .map(|index| {
                if index == 1 {
                    DEFAULT_NAME.to_owned()
                } else {
                    format!("{DEFAULT_NAME} {index}")
                }
            })
            .find(|name| !self.taken(name))
            .expect("Free world name")
    }

    /// Creates a world with a random seed and returns its trimmed name.
    pub(crate) fn create(&self, name: &str) -> Result<String, String> {
        let name = self.available(name)?;
        Database::create(
            &self.path(name),
            RandomState::new().hash_one(SystemTime::now()),
        )?;
        Ok(name.to_owned())
    }

    pub(crate) fn rename(&self, from: &str, to: &str) -> Result<String, String> {
        let to = validate(to)?;
        if to != from {
            if !to.eq_ignore_ascii_case(from) {
                self.available(to)?;
            }
            move_world(&self.path(from), &self.path(to)).map_err(|error| error.to_string())?;
        }
        Ok(to.to_owned())
    }

    pub(crate) fn delete(&self, name: &str) -> io::Result<()> {
        let path = self.path(name);
        for sidecar in SIDECARS {
            remove_if_exists(&with_suffix(&path, sidecar))?;
        }
        std::fs::remove_file(path)
    }

    fn available<'a>(&self, name: &'a str) -> Result<&'a str, String> {
        let name = validate(name)?;
        if self.taken(name) {
            return Err("A world with this name already exists".into());
        }
        Ok(name)
    }
}

/// Checks a world name, which is also its file name, and returns it trimmed.
pub(crate) fn validate(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        Err("Enter a name for the world".into())
    } else if name.chars().count() > MAX_NAME_LENGTH {
        Err(format!("Use at most {MAX_NAME_LENGTH} characters"))
    } else if name.starts_with('.') || name.ends_with('.') || !name.chars().all(valid_char) {
        Err("Names can't contain < > : \" / \\ | ? * or start or end with a dot".into())
    } else {
        Ok(name)
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut path = path.as_os_str().to_owned();
    path.push(suffix);
    path.into()
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

// Moves a closed world with the SQLite files next to it.
fn move_world(from: &Path, to: &Path) -> io::Result<()> {
    for sidecar in SIDECARS {
        let source = with_suffix(from, sidecar);
        if source.exists() {
            std::fs::rename(source, with_suffix(to, sidecar))?;
        }
    }
    std::fs::rename(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestWorlds {
        worlds: Worlds,
        directory: PathBuf,
    }
    impl TestWorlds {
        fn new(test: &str) -> Self {
            let directory = std::env::temp_dir()
                .join(format!("plaatcraft-worlds-{test}-{}", std::process::id()));
            let worlds = Worlds::with_directory(directory.join("worlds")).unwrap();
            Self { worlds, directory }
        }
    }
    impl Drop for TestWorlds {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    fn names(worlds: &Worlds) -> Vec<String> {
        let mut names: Vec<_> = worlds.list().into_iter().map(|world| world.name).collect();
        names.sort();
        names
    }

    #[test]
    fn worlds_are_created_renamed_and_deleted_as_single_files() {
        let test = TestWorlds::new("files");
        let worlds = &test.worlds;
        assert!(worlds.list().is_empty());
        assert_eq!(worlds.unused_name(), "New World");
        assert_eq!(worlds.create("  New World ").unwrap(), "New World");
        assert_eq!(worlds.unused_name(), "New World 2");
        assert!(worlds.create("new world").is_err());
        worlds.create("Island").unwrap();
        assert_eq!(names(worlds), ["Island", "New World"]);

        assert!(worlds.rename("Island", "New World").is_err());
        assert_eq!(worlds.rename("Island", "island").unwrap(), "island");
        assert_eq!(
            worlds.rename("island", "Desert Island").unwrap(),
            "Desert Island"
        );
        assert_eq!(names(worlds), ["Desert Island", "New World"]);
        assert!(Database::open(&worlds.path("Desert Island")).is_ok());

        worlds.delete("New World").unwrap();
        assert_eq!(names(worlds), ["Desert Island"]);
        assert_eq!(
            std::fs::read_dir(test.directory.join("worlds"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn names_are_trimmed_and_safe_file_names() {
        assert_eq!(validate("  Hello World "), Ok("Hello World"));
        for name in ["", "   ", ".hidden", "end.", "a/b", "a\\b", "what?", "a\tb"] {
            assert!(validate(name).is_err(), "{name}");
        }
        assert!(validate(&"x".repeat(MAX_NAME_LENGTH)).is_ok());
        assert!(validate(&"x".repeat(MAX_NAME_LENGTH + 1)).is_err());
    }
}
