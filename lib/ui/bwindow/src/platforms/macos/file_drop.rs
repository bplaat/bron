/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, msg_send};

use super::event_loop::send_event;
use super::headers::{
    NS_DRAG_OPERATION_COPY, NSFilenamesPboardType, NSPasteboardURLReadingFileURLsOnlyKey, NSString,
};
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
        let paths = pasteboard_paths(pasteboard);
        if paths.is_empty() {
            return Bool::NO;
        }
        for path in paths {
            send_event(crate::Event::Window(
                window_id,
                WindowEvent::DroppedFile(path),
            ));
        }
        Bool::YES
    }
}

// Read NSURL objects so modern file drags, escaped characters and filesystem bytes survive.
unsafe fn pasteboard_paths(pasteboard: *mut Object) -> Vec<PathBuf> {
    unsafe {
        let classes: *mut Object = msg_send![class!(NSArray), arrayWithObject:class!(NSURL)];
        let yes: *mut Object = msg_send![class!(NSNumber), numberWithBool:Bool::YES];
        let options: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObject:yes, forKey:NSPasteboardURLReadingFileURLsOnlyKey];
        let urls: *mut Object =
            msg_send![pasteboard, readObjectsForClasses:classes, options:options];
        let mut paths = Vec::new();
        let count: usize = msg_send![urls, count];
        for index in 0..count {
            let url: *mut Object = msg_send![urls, objectAtIndex:index];
            let is_file: Bool = msg_send![url, isFileURL];
            if is_file.as_bool() {
                let filename: *const std::ffi::c_char = msg_send![url, fileSystemRepresentation];
                if !filename.is_null() {
                    paths.push(PathBuf::from(OsStr::from_bytes(
                        CStr::from_ptr(filename).to_bytes(),
                    )));
                }
            }
        }
        if !paths.is_empty() {
            return paths;
        }
        // Older drag sources still publish a filename property list.
        let filenames: *mut Object =
            msg_send![pasteboard, propertyListForType:NSFilenamesPboardType];
        let count: usize = msg_send![filenames, count];
        for index in 0..count {
            let filename: NSString = msg_send![filenames, objectAtIndex:index];
            paths.push(PathBuf::from(filename.to_string()));
        }
        paths
    }
}

/// Returns the copy operation only for a drag containing local files.
///
/// # Safety
/// `sender` must be a live NSDraggingInfo object used on the AppKit thread.
pub unsafe fn file_drag_operation(sender: *mut Object) -> u64 {
    let pasteboard = unsafe { msg_send![sender, draggingPasteboard] };
    if unsafe { pasteboard_paths(pasteboard) }.is_empty() {
        0
    } else {
        NS_DRAG_OPERATION_COPY
    }
}

/// Registers a window or view for file drags
///
/// # Safety
/// `view` must be a live NSWindow or NSView used on the AppKit thread.
pub unsafe fn register_dragged_types(view: *mut Object) {
    unsafe {
        let dragged_types: *mut Object = msg_send![class!(NSMutableArray), array];
        let _: () = msg_send![dragged_types, addObject:&*NSString::new("public.file-url")];
        let _: () = msg_send![dragged_types, addObject:NSFilenamesPboardType];
        let _: () = msg_send![view, registerForDraggedTypes:dragged_types];
    }
}

#[cfg(test)]
mod tests {
    use objc2::rc::{Retained, autoreleasepool};

    use super::*;

    #[test]
    fn reads_file_urls_and_rejects_non_file_drags() {
        // SAFETY: Each pasteboard and item is retained for the test; all selectors use
        // their Foundation/AppKit signatures. No window or shared pasteboard is used.
        autoreleasepool(|_| unsafe {
            let pasteboard: Retained<Object> =
                msg_send![class!(NSPasteboard), pasteboardWithUniqueName];
            let items: Retained<Object> = msg_send![class!(NSMutableArray), new];
            for url in [
                "file:///tmp/a%20b.png",
                "file:///tmp/c%23d.pf",
                "file:///tmp/caf%C3%A9.db",
                "https://example.com/db",
            ] {
                let item: Retained<Object> = msg_send![class!(NSPasteboardItem), new];
                let kind = NSString::new(if url.starts_with("file:") {
                    "public.file-url"
                } else {
                    "public.url"
                });
                let _: Bool = msg_send![&*item, setString:&*NSString::new(url), forType:&*kind];
                let _: () = msg_send![&*items, addObject:&*item];
            }
            let _: Bool = msg_send![&*pasteboard, writeObjects:&*items];
            assert_eq!(
                pasteboard_paths(pasteboard.as_ptr()),
                [
                    PathBuf::from("/tmp/a b.png"),
                    PathBuf::from("/tmp/c#d.pf"),
                    // NSURL uses the decomposed macOS filesystem representation.
                    PathBuf::from("/tmp/cafe\u{301}.db")
                ]
            );
            let _: isize = msg_send![&*pasteboard, clearContents];
            let _: Bool = msg_send![&*pasteboard, setString:&*NSString::new("https://example.com/db"), forType:&*NSString::new("public.url")];
            assert!(pasteboard_paths(pasteboard.as_ptr()).is_empty());
            let _: isize = msg_send![&*pasteboard, clearContents];
            assert!(pasteboard_paths(pasteboard.as_ptr()).is_empty());
            let _: () = msg_send![&*pasteboard, releaseGlobally];
        });
    }

    #[test]
    fn reads_legacy_filenames() {
        // SAFETY: Only retained Foundation/AppKit objects and a private pasteboard are used.
        autoreleasepool(|_| unsafe {
            let pasteboard: Retained<Object> =
                msg_send![class!(NSPasteboard), pasteboardWithUniqueName];
            let names: *mut Object =
                msg_send![class!(NSArray), arrayWithObject:&*NSString::new("/tmp/font.pf")];
            let types: *mut Object =
                msg_send![class!(NSArray), arrayWithObject:NSFilenamesPboardType];
            let _: isize =
                msg_send![&*pasteboard, declareTypes:types, owner:std::ptr::null::<Object>()];
            let _: Bool =
                msg_send![&*pasteboard, setPropertyList:names, forType:NSFilenamesPboardType];
            assert_eq!(
                pasteboard_paths(pasteboard.as_ptr()),
                [PathBuf::from("/tmp/font.pf")]
            );
            let _: () = msg_send![&*pasteboard, releaseGlobally];
        });
    }
}
