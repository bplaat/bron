/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::PathBuf;

use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, msg_send};

use super::event_loop::send_event;
use super::headers::{NSFilenamesPboardType, NSString};
use crate::WindowEvent;

/// Sends a `DroppedFile` event for every file in a completed drag operation
///
/// # Safety
/// `sender` must be a live NSDraggingInfo object used on the AppKit thread.
pub unsafe fn perform_file_drop(sender: *mut Object) -> Bool {
    unsafe {
        let window: *mut Object = msg_send![sender, draggingDestinationWindow];
        let Some(window_id) = super::window::window_id(window) else {
            return Bool::NO;
        };
        let pasteboard: *mut Object = msg_send![sender, draggingPasteboard];
        let filenames: *mut Object =
            msg_send![pasteboard, propertyListForType:NSFilenamesPboardType];
        if filenames.is_null() {
            return Bool::NO;
        }
        let count: usize = msg_send![filenames, count];
        for index in 0..count {
            let filename: NSString = msg_send![filenames, objectAtIndex:index];
            send_event(crate::Event::Window(
                window_id,
                WindowEvent::DroppedFile(PathBuf::from(filename.to_string())),
            ));
        }
        Bool::YES
    }
}

/// Registers a window or view for file drags
///
/// # Safety
/// `view` must be a live NSWindow or NSView used on the AppKit thread.
pub unsafe fn register_dragged_types(view: *mut Object) {
    let dragged_types: *mut Object =
        unsafe { msg_send![class!(NSArray), arrayWithObject:NSFilenamesPboardType] };
    let _: () = unsafe { msg_send![view, registerForDraggedTypes:dragged_types] };
}
