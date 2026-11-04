/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Window-attached WinUI alerts and the actions resumed when the user answers.

use std::path::PathBuf;

use bwindow::WindowId;
use imageview_core::FileVersion;

use crate::edit::AfterSave;
use crate::{App, file_name};

pub(crate) struct Alert {
    title: String,
    description: String,
    purpose: Purpose,
}

enum Purpose {
    SaveChanges(AfterSave),
    Error { close: bool },
}

impl App {
    pub(crate) fn ask_save_changes(&mut self, id: WindowId, after: AfterSave) {
        let Some(window) = self.window(id) else {
            return;
        };
        let title = format!("Save changes to {}?", file_name(&window.path));
        self.queue_alert(
            id,
            Alert {
                title,
                description: "Your changes will be lost if you don't save them.".into(),
                purpose: Purpose::SaveChanges(after),
            },
        );
    }

    pub(crate) fn show_error(
        &mut self,
        id: WindowId,
        title: String,
        description: String,
        close: bool,
    ) {
        self.queue_alert(
            id,
            Alert {
                title,
                description,
                purpose: Purpose::Error { close },
            },
        );
    }

    fn queue_alert(&mut self, id: WindowId, alert: Alert) {
        let Some(window) = self.window(id) else {
            return;
        };
        window.alerts.push_back(alert);
        if window.alerts.len() == 1 {
            self.show_next_alert(id);
        }
    }

    fn show_next_alert(&mut self, id: WindowId) {
        let Some(window) = self.window(id) else {
            return;
        };
        let Some(alert) = window.alerts.front() else {
            return;
        };
        window.view.show_alert(
            id,
            &alert.title,
            &alert.description,
            matches!(alert.purpose, Purpose::SaveChanges(_)),
        );
    }

    pub(crate) fn alert_closed(&mut self, id: WindowId, result: i32) {
        let Some(window) = self.window(id) else {
            return;
        };
        window.view.close_alert();
        let Some(alert) = window.alerts.pop_front() else {
            return;
        };
        match alert.purpose {
            Purpose::SaveChanges(after) => match result {
                1 => {
                    self.save(id, false, after);
                }
                2 => {
                    self.leave_editing(id, true);
                    self.continue_after_save(id, after);
                }
                _ => {}
            },
            Purpose::Error { close: true } => self.close_window(id),
            Purpose::Error { close: false } => {}
        }
        self.show_next_alert(id);
    }

    pub(crate) fn open_error(
        &mut self,
        path: PathBuf,
        version: Option<FileVersion>,
        index: Option<usize>,
        error: String,
    ) {
        let (id, close) = if let Some(index) = index {
            let window = &mut self.windows[index];
            window.opening = None;
            (window.window.id(), true)
        } else if let Some(window) = self.active_window() {
            (window.window.id(), false)
        } else {
            self.open_window(path.clone(), version, None, None);
            (self.windows.last().expect("error window").window.id(), true)
        };
        self.show_error(
            id,
            format!("Could not open {}", file_name(&path)),
            error,
            close,
        );
    }
}
