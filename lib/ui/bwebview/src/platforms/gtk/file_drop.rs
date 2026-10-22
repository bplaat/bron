/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::null;

use bwindow::ffi::gtk_dropped_paths;

use super::headers::*;
use super::webview::WebviewData;
use crate::WindowEvent;

/// The paths of the drag in progress, collected before the drop is confirmed
#[derive(Default)]
pub(super) struct FileDropState {
    paths: Option<Vec<PathBuf>>,
    leaving: bool,
}

/// Reports native file drags on the web view instead of letting WebKit navigate to them
pub(super) unsafe fn connect_signals(webview: *mut WebKitWebView, data: &WebviewData) {
    let data = data as *const WebviewData as *const c_void;
    unsafe {
        g_signal_connect_data(
            webview as *mut GObject,
            c"drag-leave".as_ptr(),
            webview_on_drag_leave as *const c_void,
            data,
            null(),
            G_CONNECT_DEFAULT,
        );
        g_signal_connect_data(
            webview as *mut GObject,
            c"drag-drop".as_ptr(),
            webview_on_drag_drop as *const c_void,
            data,
            null(),
            G_CONNECT_DEFAULT,
        );
        g_signal_connect_data(
            webview as *mut GObject,
            c"drag-data-received".as_ptr(),
            webview_on_drag_data_received as *const c_void,
            data,
            null(),
            G_CONNECT_DEFAULT,
        );
    }
}

extern "C" fn webview_on_drag_leave(
    _webview: *mut WebKitWebView,
    _context: *mut GdkDragContext,
    _time: u32,
    data: &WebviewData,
) {
    data.file_drop.borrow_mut().leaving = true;
}

extern "C" fn webview_on_drag_drop(
    _webview: *mut WebKitWebView,
    context: *mut GdkDragContext,
    _x: i32,
    _y: i32,
    time: u32,
    data: &WebviewData,
) -> i32 {
    let data = unsafe { WebviewData::retain(data) };
    let paths = {
        let mut drop_state = data.file_drop.borrow_mut();
        if drop_state.leaving {
            drop_state.leaving = false;
            drop_state.paths.take()
        } else {
            None
        }
    };
    if let Some(paths) = paths {
        unsafe { gtk_drag_finish(context, 1, 0, time) };
        let sender = data.attachment.event_sender();
        for path in paths {
            sender.send(WindowEvent::DroppedFile(path));
        }
        return 1;
    }
    0
}

extern "C" fn webview_on_drag_data_received(
    _webview: *mut WebKitWebView,
    _context: *mut GdkDragContext,
    _x: i32,
    _y: i32,
    selection_data: *mut GtkSelectionData,
    info: u32,
    _time: u32,
    data: &WebviewData,
) {
    // WebKitGTK registers URI lists as target info 2. Other data requests are
    // used internally by the web view and are not native file drags.
    if info != 2 {
        return;
    }

    let paths = unsafe { gtk_dropped_paths(selection_data.cast()) };
    data.file_drop.borrow_mut().paths = Some(paths);
    data.file_drop.borrow_mut().leaving = false;
}
