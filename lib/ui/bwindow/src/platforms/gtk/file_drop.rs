/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, OsStr, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use super::headers::*;
use crate::{WindowEvent, WindowEventSender};

/// Read local file paths from a GTK URI-list selection.
///
/// # Safety
/// `selection` must be a live GtkSelectionData during its GTK callback.
pub unsafe fn gtk_dropped_paths(selection: *mut c_void) -> Vec<PathBuf> {
    let uris = unsafe { gtk_selection_data_get_uris(selection.cast()) };
    if uris.is_null() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    let mut uri = uris;
    unsafe {
        while !(*uri).is_null() {
            let filename = g_filename_from_uri(*uri, null_mut(), null_mut());
            if !filename.is_null() {
                paths.push(PathBuf::from(OsStr::from_bytes(
                    CStr::from_ptr(filename).to_bytes(),
                )));
                g_free(filename.cast());
            }
            uri = uri.add(1);
        }
        g_strfreev(uris);
    }
    paths
}

/// Install default file-drop handling for a plain GTK window or drawable content.
/// The widget owns the signal data; the sender does not keep its window alive.
///
/// # Safety
/// `widget` must be a live GtkWidget on the GTK UI thread.
pub unsafe fn gtk_connect_file_drop(widget: *mut c_void, sender: WindowEventSender) {
    let widget = widget.cast::<GtkWidget>();
    unsafe {
        gtk_drag_dest_set(widget, 7, null(), 0, 2);
        gtk_drag_dest_add_uri_targets(widget);
        g_signal_connect_data(
            widget.cast(),
            c"drag-data-received".as_ptr(),
            received as *const c_void,
            Box::into_raw(Box::new(sender)).cast(),
            release as *const c_void,
            G_CONNECT_DEFAULT,
        );
    }
}

extern "C" fn release(data: *mut WindowEventSender, _: *mut c_void) {
    drop(unsafe { Box::from_raw(data) });
}

extern "C" fn received(
    _: *mut GtkWidget,
    _: *mut GdkDragContext,
    _: i32,
    _: i32,
    selection: *mut GtkSelectionData,
    _: u32,
    _: u32,
    sender: &WindowEventSender,
) {
    let sender = sender.clone();
    let paths = unsafe { gtk_dropped_paths(selection.cast()) };
    for path in paths {
        sender.send(WindowEvent::DroppedFile(path));
    }
}
