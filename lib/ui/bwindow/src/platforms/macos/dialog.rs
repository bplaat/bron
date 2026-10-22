/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, msg_send};

use super::headers::*;

const ESCAPE_KEY_EQUIVALENT: &str = "\u{1b}";

fn escape_key_equivalent() -> *mut Object {
    ns_string!(ESCAPE_KEY_EQUIVALENT)
}

pub(crate) struct PlatformMessageDialog;

impl crate::MessageDialogInterface for PlatformMessageDialog {
    fn show(dialog: crate::MessageDialog<'_>) -> crate::MessageDialogResult {
        unsafe {
            let alert: Retained<Object> = msg_send![class!(NSAlert), new];
            let style = match dialog.level {
                crate::MessageLevel::Info => NS_ALERT_STYLE_INFORMATIONAL,
                crate::MessageLevel::Warning => NS_ALERT_STYLE_WARNING,
                crate::MessageLevel::Error => NS_ALERT_STYLE_CRITICAL,
            };
            let _: () = msg_send![&alert, setAlertStyle:style];
            if dialog.title.is_empty() {
                let _: () = msg_send![&alert, setMessageText:&*NSString::new(&dialog.description)];
            } else {
                let _: () = msg_send![&alert, setMessageText:&*NSString::new(&dialog.title)];
                let _: () =
                    msg_send![&alert, setInformativeText:&*NSString::new(&dialog.description)];
            }

            let labels = crate::dialog::message_button_labels(&dialog.buttons);
            for label in &labels {
                let _: *mut Object = msg_send![&alert, addButtonWithTitle:&*NSString::new(label)];
            }
            if labels.len() > 1 {
                let buttons: *mut Object = msg_send![&alert, buttons];
                let cancel: *mut Object = msg_send![buttons, lastObject];
                let _: () = msg_send![cancel, setKeyEquivalent:escape_key_equivalent()];
            }

            let parent = dialog
                .parent
                .map(|window| window.0.window.as_ptr())
                .unwrap_or_else(|| msg_send![NSApp, keyWindow]);
            let response: i64 = if parent.is_null() {
                msg_send![&alert, runModal]
            } else {
                let block = RcBlock::new(move |response: i64| {
                    let _: () = msg_send![NSApp, stopModalWithCode:response];
                });
                let _: () =
                    msg_send![&alert, beginSheetModalForWindow:parent, completionHandler:&*block];
                let alert_window: *mut Object = msg_send![&alert, window];
                msg_send![NSApp, runModalForWindow:alert_window]
            };
            let index = response.saturating_sub(NS_ALERT_FIRST_BUTTON_RETURN) as usize;
            crate::dialog::message_dialog_result(&dialog.buttons, index)
        }
    }
}

pub(crate) struct PlatformFileDialog;

impl crate::FileDialogInterface for PlatformFileDialog {
    fn pick_file(dialog: crate::FileDialog<'_>) -> Option<std::path::PathBuf> {
        unsafe {
            let panel: *mut Object = msg_send![class!(NSOpenPanel), openPanel];
            let _: () = msg_send![panel, setCanChooseFiles: Bool::YES];
            let _: () = msg_send![panel, setCanChooseDirectories: Bool::NO];
            let _: () = msg_send![panel, setAllowsMultipleSelection: Bool::NO];
            setup_ns_panel(panel, &dialog);
            let result: i64 = run_panel_modal(panel, dialog.parent);
            if result == NS_MODAL_RESPONSE_OK {
                let urls: *mut Object = msg_send![panel, URLs];
                let url: *mut Object = msg_send![urls, objectAtIndex: 0usize];
                let path: NSString = msg_send![url, path];
                Some(std::path::PathBuf::from(path.to_string()))
            } else {
                None
            }
        }
    }

    fn pick_files(dialog: crate::FileDialog<'_>) -> Option<Vec<std::path::PathBuf>> {
        unsafe {
            let panel: *mut Object = msg_send![class!(NSOpenPanel), openPanel];
            let _: () = msg_send![panel, setCanChooseFiles: Bool::YES];
            let _: () = msg_send![panel, setCanChooseDirectories: Bool::NO];
            let _: () = msg_send![panel, setAllowsMultipleSelection: Bool::YES];
            setup_ns_panel(panel, &dialog);
            let result: i64 = run_panel_modal(panel, dialog.parent);
            if result == NS_MODAL_RESPONSE_OK {
                let urls: *mut Object = msg_send![panel, URLs];
                let count: usize = msg_send![urls, count];
                let paths: Vec<_> = (0..count)
                    .map(|i| {
                        let url: *mut Object = msg_send![urls, objectAtIndex: i];
                        let path: NSString = msg_send![url, path];
                        std::path::PathBuf::from(path.to_string())
                    })
                    .collect();
                if paths.is_empty() { None } else { Some(paths) }
            } else {
                None
            }
        }
    }

    fn save_file(dialog: crate::FileDialog<'_>) -> Option<std::path::PathBuf> {
        unsafe {
            let panel: *mut Object = msg_send![class!(NSSavePanel), savePanel];
            setup_ns_panel(panel, &dialog);
            if let Some(filename) = &dialog.filename {
                let _: () = msg_send![panel, setNameFieldStringValue: &*NSString::new(filename)];
            }
            let result: i64 = run_panel_modal(panel, dialog.parent);
            if result == NS_MODAL_RESPONSE_OK {
                let url: *mut Object = msg_send![panel, URL];
                let path: NSString = msg_send![url, path];
                Some(std::path::PathBuf::from(path.to_string()))
            } else {
                None
            }
        }
    }
}

unsafe fn setup_ns_panel(panel: *mut Object, dialog: &crate::FileDialog<'_>) {
    unsafe {
        if let Some(title) = &dialog.title {
            let _: () = msg_send![panel, setTitle: &*NSString::new(title)];
        }
        if let Some(dir) = &dialog.directory {
            let url: *mut Object =
                msg_send![class!(NSURL), fileURLWithPath: &*NSString::new(dir.to_string_lossy())];
            let _: () = msg_send![panel, setDirectoryURL: url];
        }
        if !dialog.filters.is_empty() {
            let arr: Retained<Object> = msg_send![class!(NSMutableArray), new];
            for filter in &dialog.filters {
                for ext in &filter.extensions {
                    let _: () = msg_send![&arr, addObject: &*NSString::new(ext)];
                }
            }
            // setAllowedFileTypes: is deprecated in macOS 12 but still functional
            let _: () = msg_send![panel, setAllowedFileTypes: arr.as_ptr()];
        }
    }
}

unsafe fn run_panel_modal(
    panel: *mut Object,
    parent: Option<&super::window::PlatformWindow>,
) -> i64 {
    unsafe {
        let parent = parent
            .map(|window| window.0.window.as_ptr())
            .unwrap_or_else(|| msg_send![NSApp, keyWindow]);
        if !parent.is_null() {
            // Show as a sheet attached to the active window
            let block = RcBlock::new(move |response: i64| {
                let _: () = msg_send![NSApp, stopModalWithCode: response];
            });
            let _: () =
                msg_send![panel, beginSheetModalForWindow: parent, completionHandler: &*block];
            msg_send![NSApp, runModalForWindow: panel]
        } else {
            msg_send![panel, runModal]
        }
    }
}
