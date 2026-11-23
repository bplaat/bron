/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::Arc;

use bwindow::{EventLoopProxy, Window, WindowsProgressBarState};
use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;
use crate::engine::CleanupResult;

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum IpcRequest {
    Initialize,
    RestartElevated,
    StartScan,
    CancelScan,
    StartClean { cleanup_ids: Vec<String> },
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum IpcPush<'a> {
    Catalog {
        catalog: &'a Catalog,
        disk_free: Option<u64>,
        is_administrator: bool,
    },
    OperationStarted {
        operation: &'a str,
        total: usize,
    },
    CleanupProgress {
        operation: &'a str,
        cleanup_id: &'a str,
        index: usize,
        total: usize,
    },
    ScanResult {
        result: CleanupResult,
    },
    ScanFinished {
        cancelled: bool,
    },
    CleanResult {
        result: CleanupResult,
    },
    CleanFinished {
        recovered_bytes: u64,
    },
    DiskFreeUpdated {
        disk_free: Option<u64>,
    },
    FatalError {
        message: String,
    },
    ElevationError {
        message: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "state", content = "progress", rename_all = "camelCase")]
pub(crate) enum ProgressBarState {
    None,
    Indeterminate,
    Normal(f32),
    Error(f32),
}

pub(crate) const PROGRESS_EVENT_PREFIX: &str = "binman:progress:";

pub(crate) fn send_progress(proxy: &EventLoopProxy<crate::AppEvent>, state: ProgressBarState) {
    if let Ok(json) = serde_json::to_string(&state) {
        let _ = proxy.send_user_event(crate::AppEvent::UserEvent(format!(
            "{PROGRESS_EVENT_PREFIX}{json}"
        )));
    }
}

pub(crate) fn update_progress(window: &mut Window, state: ProgressBarState) {
    match state {
        ProgressBarState::None => {
            window.windows_set_progress_bar(None, WindowsProgressBarState::Normal);
        }
        ProgressBarState::Indeterminate => {
            window.windows_set_progress_bar(Some(0.0), WindowsProgressBarState::Indeterminate);
        }
        ProgressBarState::Normal(progress) => {
            window.windows_set_progress_bar(Some(progress), WindowsProgressBarState::Normal);
        }
        ProgressBarState::Error(progress) => {
            window.windows_set_progress_bar(Some(progress), WindowsProgressBarState::Error);
        }
    }
}

pub(crate) fn send_push(proxy: &Arc<EventLoopProxy<crate::AppEvent>>, push: IpcPush<'_>) {
    match serde_json::to_string(&push) {
        Ok(json) => {
            let _ = proxy.send_user_event(crate::AppEvent::UserEvent(json));
        }
        Err(error) => {
            let fallback = serde_json::to_string(&IpcPush::FatalError {
                message: error.to_string(),
            })
            .expect("fatal error should serialize");
            let _ = proxy.send_user_event(crate::AppEvent::UserEvent(fallback));
        }
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_request_accepts_browser_field_names() {
        let request = serde_json::from_str::<IpcRequest>(
            r#"{"type":"startClean","cleanupIds":["windows-user-temp"]}"#,
        )
        .expect("browser cleanup request should deserialize");

        match request {
            IpcRequest::StartClean { cleanup_ids } => {
                assert_eq!(cleanup_ids, ["windows-user-temp"]);
            }
            _ => panic!("expected cleanup request"),
        }
    }

    #[test]
    fn progress_push_uses_browser_field_names() {
        let json = serde_json::to_string(&IpcPush::CleanupProgress {
            operation: "scan",
            cleanup_id: "windows-user-temp",
            index: 0,
            total: 1,
        })
        .expect("progress push should serialize");

        assert!(json.contains(r#""type":"cleanupProgress""#));
        assert!(json.contains(r#""cleanupId":"windows-user-temp""#));
        assert!(!json.contains("cleanup_id"));
    }
}
