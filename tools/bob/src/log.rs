/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::str::FromStr;
use std::time::Duration;

// MARK: LogEntry
pub(crate) struct LogEntry {
    pub path: String,
    pub mtime: Duration,
    pub hash: Option<[u8; 20]>,
}

impl FromStr for LogEntry {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(' ').collect();
        if parts.len() != 3 && parts.len() != 4 {
            return Err("Invalid log entry format".to_string());
        }

        let path = parts[0].to_string();

        let mtime_secs = parts[1]
            .parse::<u64>()
            .map_err(|_| "Invalid modified time".to_string())?;
        let mtime_nanos = parts[2]
            .parse::<u32>()
            .map_err(|_| "Invalid modified time".to_string())?;

        let hash = if parts.len() == 4 {
            let hash_str = parts[3];
            if hash_str.len() != 40 {
                return Err("Invalid hash format".to_string());
            }
            let mut hash = [0u8; 20];
            for (i, byte) in hash.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&hash_str[i * 2..i * 2 + 2], 16)
                    .map_err(|_| "Invalid hash format".to_string())?;
            }
            Some(hash)
        } else {
            None
        };

        Ok(LogEntry {
            path,
            mtime: Duration::new(mtime_secs, mtime_nanos),
            hash,
        })
    }
}

impl Display for LogEntry {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.path,
            self.mtime.as_secs(),
            self.mtime.subsec_nanos()
        )?;
        if let Some(hash) = &self.hash {
            write!(f, " ")?;
            for byte in hash {
                write!(f, "{byte:02x}")?;
            }
        }
        Ok(())
    }
}

// MARK: Log
pub(crate) struct Log {
    file: File,
    entries: HashMap<String, LogEntry>,
}

impl Log {
    pub(crate) fn new(path: &str) -> Self {
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)
            .unwrap_or_else(|_| panic!("Can't open file: {path}"));

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .unwrap_or_else(|_| panic!("Can't read file: {path}"));

        let entries = contents
            .lines()
            .map(|line| line.parse::<LogEntry>())
            .map(|entry| entry.map(|entry| (entry.path.clone(), entry)))
            .collect::<Result<HashMap<_, _>, _>>()
            .unwrap_or_default();

        // Rewrite the file when it is corrupt or has superseded entries, so it doesn't keep growing
        if entries.len() != contents.lines().count() {
            file.set_len(0)
                .unwrap_or_else(|_| panic!("Can't truncate file: {path}"));
            let mut writer = BufWriter::new(&file);
            for entry in entries.values() {
                writeln!(writer, "{entry}").unwrap_or_else(|_| panic!("Can't write to file"));
            }
            writer
                .flush()
                .unwrap_or_else(|_| panic!("Can't write to file"));
        }
        Log { file, entries }
    }

    pub(crate) fn get(&self, path: &str) -> Option<&LogEntry> {
        self.entries.get(path)
    }

    pub(crate) fn add(&mut self, entry: LogEntry) {
        writeln!(self.file, "{entry}").unwrap_or_else(|_| panic!("Can't write to file"));
        self.entries.insert(entry.path.clone(), entry);
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn compacts_superseded_entries() {
        let path = std::env::temp_dir().join(format!("bob-log-{}.log", std::process::id()));
        fs::write(&path, "a 1 0\nb 2 0\na 3 0\n").expect("write log");
        let path = path.to_str().expect("UTF-8 path");

        let log = Log::new(path);
        assert_eq!(log.get("a").map(|entry| entry.mtime.as_secs()), Some(3));
        assert_eq!(
            fs::read_to_string(path).expect("read log").lines().count(),
            2
        );

        fs::remove_file(path).expect("remove log");
    }
}
