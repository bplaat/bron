/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::platforms::{PlatformFileDialog, PlatformMessageDialog};

// MARK: Message dialogs
/// Message dialog level
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageLevel {
    /// Informational message
    #[default]
    Info,
    /// Warning message
    Warning,
    /// Error message
    Error,
}

/// Message dialog buttons
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MessageButtons {
    /// OK button
    #[default]
    Ok,
    /// OK and Cancel buttons
    OkCancel,
    /// Yes and No buttons
    YesNo,
    /// Yes, No, and Cancel buttons
    YesNoCancel,
    /// One custom button
    OkCustom(String),
    /// Two custom buttons
    OkCancelCustom(String, String),
    /// Three custom buttons
    YesNoCancelCustom(String, String, String),
}

/// Message dialog result
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum MessageDialogResult {
    /// Yes was selected
    Yes,
    /// No was selected
    No,
    /// OK was selected
    #[default]
    Ok,
    /// The dialog was cancelled
    Cancel,
    /// A custom button was selected
    Custom(String),
}

#[cfg(not(windows))]
pub(crate) fn message_button_labels(buttons: &MessageButtons) -> Vec<&str> {
    match buttons {
        MessageButtons::Ok => vec!["OK"],
        MessageButtons::OkCancel => vec!["OK", "Cancel"],
        MessageButtons::YesNo => vec!["Yes", "No"],
        MessageButtons::YesNoCancel => vec!["Yes", "No", "Cancel"],
        MessageButtons::OkCustom(ok) => vec![ok],
        MessageButtons::OkCancelCustom(ok, cancel) => vec![ok, cancel],
        MessageButtons::YesNoCancelCustom(yes, no, cancel) => vec![yes, no, cancel],
    }
}

pub(crate) fn message_dialog_result(buttons: &MessageButtons, index: usize) -> MessageDialogResult {
    match (buttons, index) {
        (MessageButtons::Ok, 0) => MessageDialogResult::Ok,
        (MessageButtons::OkCancel, 0) => MessageDialogResult::Ok,
        (MessageButtons::OkCancel, _) => MessageDialogResult::Cancel,
        (MessageButtons::YesNo, 0) => MessageDialogResult::Yes,
        (MessageButtons::YesNo, _) => MessageDialogResult::No,
        (MessageButtons::YesNoCancel, 0) => MessageDialogResult::Yes,
        (MessageButtons::YesNoCancel, 1) => MessageDialogResult::No,
        (MessageButtons::YesNoCancel, _) => MessageDialogResult::Cancel,
        (MessageButtons::OkCustom(ok), 0) => MessageDialogResult::Custom(ok.clone()),
        (MessageButtons::OkCancelCustom(ok, _), 0) => MessageDialogResult::Custom(ok.clone()),
        (MessageButtons::OkCancelCustom(_, cancel), _) => {
            MessageDialogResult::Custom(cancel.clone())
        }
        (MessageButtons::YesNoCancelCustom(yes, _, _), 0) => {
            MessageDialogResult::Custom(yes.clone())
        }
        (MessageButtons::YesNoCancelCustom(_, no, _), 1) => MessageDialogResult::Custom(no.clone()),
        (MessageButtons::YesNoCancelCustom(_, _, cancel), _) => {
            MessageDialogResult::Custom(cancel.clone())
        }
        _ => MessageDialogResult::Cancel,
    }
}

pub(crate) trait MessageDialogInterface {
    fn show(dialog: MessageDialog<'_>) -> MessageDialogResult;
}

/// Native message dialog builder
#[derive(Default)]
pub struct MessageDialog<'a> {
    pub(crate) parent: Option<&'a crate::platforms::PlatformWindow>,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) level: MessageLevel,
    pub(crate) buttons: MessageButtons,
}

impl<'a> MessageDialog<'a> {
    /// Create a message dialog
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the parent window
    pub const fn parent(mut self, window: &'a crate::Window) -> Self {
        self.parent = Some(&window.platform);
        self
    }

    /// Set the dialog title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the dialog description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the message level
    pub const fn level(mut self, level: MessageLevel) -> Self {
        self.level = level;
        self
    }

    /// Set the dialog buttons
    pub fn buttons(mut self, buttons: MessageButtons) -> Self {
        self.buttons = buttons;
        self
    }

    /// Show the dialog and return the selected button
    pub fn show(self) -> MessageDialogResult {
        PlatformMessageDialog::show(self)
    }
}

// MARK: File dialogs
pub(crate) struct FileDialogFilter {
    #[cfg_attr(
        target_os = "macos",
        allow(dead_code, reason = "AppKit only filters on extensions")
    )]
    pub(crate) name: String,
    pub(crate) extensions: Vec<String>,
}

pub(crate) trait FileDialogInterface {
    fn pick_file(dialog: FileDialog<'_>) -> Option<std::path::PathBuf>;
    fn pick_files(dialog: FileDialog<'_>) -> Option<Vec<std::path::PathBuf>>;
    fn save_file(dialog: FileDialog<'_>) -> Option<std::path::PathBuf>;
}

/// File dialog builder
#[derive(Default)]
pub struct FileDialog<'a> {
    pub(crate) parent: Option<&'a crate::platforms::PlatformWindow>,
    pub(crate) title: Option<String>,
    pub(crate) directory: Option<std::path::PathBuf>,
    pub(crate) filename: Option<String>,
    pub(crate) filters: Vec<FileDialogFilter>,
}

impl<'a> FileDialog<'a> {
    /// Create new file dialog
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the parent window
    pub const fn parent(mut self, window: &'a crate::Window) -> Self {
        self.parent = Some(&window.platform);
        self
    }

    /// Set dialog title
    pub fn title(mut self, title: impl AsRef<str>) -> Self {
        self.title = Some(title.as_ref().to_string());
        self
    }

    /// Set starting directory
    pub fn directory(mut self, path: impl AsRef<std::path::Path>) -> Self {
        self.directory = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set default file name (used for save dialogs)
    pub fn file_name(mut self, filename: impl AsRef<str>) -> Self {
        self.filename = Some(filename.as_ref().to_string());
        self
    }

    /// Add a file filter (name + list of extensions without dot)
    pub fn add_filter(mut self, name: impl AsRef<str>, extensions: &[impl AsRef<str>]) -> Self {
        self.filters.push(FileDialogFilter {
            name: name.as_ref().to_string(),
            extensions: extensions.iter().map(|e| e.as_ref().to_string()).collect(),
        });
        self
    }

    /// Open a single-file picker dialog
    pub fn pick_file(self) -> Option<std::path::PathBuf> {
        PlatformFileDialog::pick_file(self)
    }

    /// Open a multi-file picker dialog
    pub fn pick_files(self) -> Option<Vec<std::path::PathBuf>> {
        PlatformFileDialog::pick_files(self)
    }

    /// Open a save-file dialog
    pub fn save_file(self) -> Option<std::path::PathBuf> {
        PlatformFileDialog::save_file(self)
    }
}
