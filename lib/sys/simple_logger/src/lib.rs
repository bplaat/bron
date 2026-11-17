/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [simple_logger](https://crates.io/crates/simple_logger) crate

use std::env;
use std::io::{self, IsTerminal};

use jiff::Timestamp;
use log::{Level, LevelFilter, Metadata, Record};

// MARK: SimpleLogger
/// Simple logger that logs to stdout
pub struct SimpleLogger {
    max_level: LevelFilter,
    use_colors: bool,
}

impl Default for SimpleLogger {
    fn default() -> Self {
        Self {
            max_level: LevelFilter::Info,
            use_colors: io::stdout().is_terminal()
                && env::var_os("NO_COLOR").is_none()
                && env::var_os("CI").is_none(),
        }
    }
}

impl SimpleLogger {
    /// Create a new logger with debug log level
    pub fn new() -> Self {
        SimpleLogger::default()
    }

    /// Create a new logger with the given log level
    pub fn new_with_level(level: LevelFilter) -> Self {
        SimpleLogger {
            max_level: level,
            ..Default::default()
        }
    }

    /// Set global logger to this logger
    pub fn init(self) -> Result<(), log::SetLoggerError> {
        #[cfg(windows)]
        if self.use_colors {
            _ = enable_ansi_support::enable_ansi_support();
        }

        log::set_max_level(self.max_level);
        log::set_boxed_logger(Box::new(self))
    }
}

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.max_level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let level = if self.use_colors {
                match record.level() {
                    Level::Error => "\x1b[31mE\x1b[0m", // Red
                    Level::Warn => "\x1b[33mW\x1b[0m",  // Yellow
                    Level::Info => "\x1b[32mI\x1b[0m",  // Green
                    Level::Debug => "\x1b[34mD\x1b[0m", // Blue
                    Level::Trace => "\x1b[35mT\x1b[0m", // Magenta
                }
            } else {
                match record.level() {
                    Level::Error => "E",
                    Level::Warn => "W",
                    Level::Info => "I",
                    Level::Debug => "D",
                    Level::Trace => "T",
                }
            };

            println!(
                "{} {} {}: {}",
                Timestamp::now(),
                level,
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

// MARK: Utils

/// Initialize the global logger with default settings
pub fn init() -> Result<(), log::SetLoggerError> {
    SimpleLogger::new().init()
}

/// Initialize the global logger with the given log level
pub fn init_with_level(level: LevelFilter) -> Result<(), log::SetLoggerError> {
    SimpleLogger::new_with_level(level).init()
}
