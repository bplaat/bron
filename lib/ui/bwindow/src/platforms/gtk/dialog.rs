/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr::null_mut;

use super::headers::*;

const FIRST_CUSTOM_RESPONSE: i32 = 1;

pub(crate) struct PlatformMessageDialog;

impl crate::MessageDialogInterface for PlatformMessageDialog {
    fn show(dialog: crate::MessageDialog<'_>) -> crate::MessageDialogResult {
        unsafe {
            let parent = dialog
                .parent
                .map(|window| window.0.window)
                .unwrap_or(null_mut());
            let title =
                CString::new(dialog.title.as_str()).expect("Can't convert title to CString");
            let description = CString::new(dialog.description.as_str())
                .expect("Can't convert description to CString");
            let message_type = match dialog.level {
                crate::MessageLevel::Info => GTK_MESSAGE_INFO,
                crate::MessageLevel::Warning => GTK_MESSAGE_WARNING,
                crate::MessageLevel::Error => GTK_MESSAGE_ERROR,
            };
            let message = gtk_message_dialog_new(
                parent,
                GTK_DIALOG_MODAL | GTK_DIALOG_DESTROY_WITH_PARENT,
                message_type,
                GTK_BUTTONS_NONE,
                c"%s".as_ptr(),
                description.as_ptr(),
            );
            if !dialog.title.is_empty() {
                gtk_window_set_title(message as *mut GtkWindow, title.as_ptr());
            }

            let labels = crate::dialog::message_button_labels(&dialog.buttons);
            for (index, label) in labels.iter().enumerate() {
                let label = CString::new(*label).expect("Can't convert button to CString");
                gtk_dialog_add_button(
                    message as *mut GtkDialog,
                    label.as_ptr(),
                    FIRST_CUSTOM_RESPONSE + index as i32,
                );
            }
            gtk_dialog_set_default_response(message as *mut GtkDialog, FIRST_CUSTOM_RESPONSE);

            let response = gtk_dialog_run(message as *mut GtkDialog);
            gtk_widget_destroy(message);
            if response < FIRST_CUSTOM_RESPONSE {
                crate::MessageDialogResult::Cancel
            } else {
                crate::dialog::message_dialog_result(
                    &dialog.buttons,
                    (response - FIRST_CUSTOM_RESPONSE) as usize,
                )
            }
        }
    }
}

pub(crate) struct PlatformFileDialog;

impl crate::FileDialogInterface for PlatformFileDialog {
    fn pick_file(dialog: crate::FileDialog<'_>) -> Option<PathBuf> {
        open_files_impl(dialog, false).map(|mut v| v.remove(0))
    }

    fn pick_files(dialog: crate::FileDialog<'_>) -> Option<Vec<PathBuf>> {
        open_files_impl(dialog, true)
    }

    fn save_file(dialog: crate::FileDialog<'_>) -> Option<PathBuf> {
        unsafe {
            let title = CString::new(dialog.title.as_deref().unwrap_or("Save File"))
                .expect("Can't convert to CString");
            let parent = dialog
                .parent
                .map(|window| window.0.window)
                .unwrap_or(null_mut());

            let chooser = cfg_select! {
                gtk3_20 => gtk_file_chooser_native_new(
                    title.as_ptr(),
                    parent,
                    GTK_FILE_CHOOSER_ACTION_SAVE,
                    c"_Save".as_ptr(),
                    c"_Cancel".as_ptr(),
                ) as *mut c_void,
                _ => gtk_file_chooser_dialog_new(
                    title.as_ptr(),
                    parent,
                    GTK_FILE_CHOOSER_ACTION_SAVE,
                    c"_Cancel".as_ptr(),
                    GTK_RESPONSE_CANCEL,
                    c"_Save".as_ptr(),
                    GTK_RESPONSE_ACCEPT,
                    null::<c_char>(),
                ) as *mut c_void,
            };

            if let Some(dir) = &dialog.directory {
                let dir_c =
                    CString::new(dir.to_string_lossy().as_ref()).expect("Can't convert to CString");
                gtk_file_chooser_set_current_folder(chooser, dir_c.as_ptr());
            }
            if let Some(filename) = &dialog.filename {
                let name_c = CString::new(filename.as_str()).expect("Can't convert to CString");
                gtk_file_chooser_set_current_name(chooser, name_c.as_ptr());
            }
            add_gtk_filters(chooser, &dialog.filters);

            let result = cfg_select! {
                gtk3_20 => gtk_native_dialog_run(chooser as *mut GtkNativeDialog),
                _ => gtk_dialog_run(chooser as *mut GtkDialog),
            };

            let path = if result == GTK_RESPONSE_ACCEPT {
                let raw = gtk_file_chooser_get_filename(chooser);
                if !raw.is_null() {
                    let p = PathBuf::from(CStr::from_ptr(raw).to_string_lossy().into_owned());
                    g_free(raw as *mut c_void);
                    Some(p)
                } else {
                    None
                }
            } else {
                None
            };

            cfg_select! {
                gtk3_20 => g_object_unref(chooser as *mut GObject),
                _ => gtk_widget_destroy(chooser as *mut GtkWidget),
            }
            path
        }
    }
}

fn open_files_impl(dialog: crate::FileDialog<'_>, multiple: bool) -> Option<Vec<PathBuf>> {
    unsafe {
        let title = CString::new(dialog.title.as_deref().unwrap_or("Open File"))
            .expect("Can't convert to CString");
        let parent = dialog
            .parent
            .map(|window| window.0.window)
            .unwrap_or(null_mut());

        let chooser = cfg_select! {
            gtk3_20 => gtk_file_chooser_native_new(
                title.as_ptr(),
                parent,
                GTK_FILE_CHOOSER_ACTION_OPEN,
                c"_Open".as_ptr(),
                c"_Cancel".as_ptr(),
            ) as *mut c_void,
            _ => gtk_file_chooser_dialog_new(
                title.as_ptr(),
                parent,
                GTK_FILE_CHOOSER_ACTION_OPEN,
                c"_Cancel".as_ptr(),
                GTK_RESPONSE_CANCEL,
                c"_Open".as_ptr(),
                GTK_RESPONSE_ACCEPT,
                null::<c_char>(),
            ) as *mut c_void,
        };

        if multiple {
            gtk_file_chooser_set_select_multiple(chooser, true);
        }
        if let Some(dir) = &dialog.directory {
            let dir_c =
                CString::new(dir.to_string_lossy().as_ref()).expect("Can't convert to CString");
            gtk_file_chooser_set_current_folder(chooser, dir_c.as_ptr());
        }
        add_gtk_filters(chooser, &dialog.filters);

        let result = cfg_select! {
            gtk3_20 => gtk_native_dialog_run(chooser as *mut GtkNativeDialog),
            _ => gtk_dialog_run(chooser as *mut GtkDialog),
        };

        let paths = if result == GTK_RESPONSE_ACCEPT {
            if multiple {
                let slist = gtk_file_chooser_get_filenames(chooser);
                if slist.is_null() {
                    None
                } else {
                    let mut paths = Vec::new();
                    let mut node = slist;
                    while !node.is_null() {
                        let raw = (*node).data as *const c_char;
                        if !raw.is_null() {
                            paths.push(PathBuf::from(
                                CStr::from_ptr(raw).to_string_lossy().into_owned(),
                            ));
                        }
                        node = (*node).next;
                    }
                    g_slist_free_full(slist, g_free);
                    if paths.is_empty() { None } else { Some(paths) }
                }
            } else {
                let raw = gtk_file_chooser_get_filename(chooser);
                if !raw.is_null() {
                    let p = PathBuf::from(CStr::from_ptr(raw).to_string_lossy().into_owned());
                    g_free(raw as *mut c_void);
                    Some(vec![p])
                } else {
                    None
                }
            }
        } else {
            None
        };

        cfg_select! {
            gtk3_20 => g_object_unref(chooser as *mut GObject),
            _ => gtk_widget_destroy(chooser as *mut GtkWidget),
        }
        paths
    }
}

unsafe fn add_gtk_filters(chooser: *mut c_void, filters: &[crate::FileDialogFilter]) {
    for filter in filters {
        let f = unsafe { gtk_file_filter_new() };
        let name = CString::new(filter.name.as_str()).expect("Can't convert to CString");
        unsafe { gtk_file_filter_set_name(f, name.as_ptr()) };
        for ext in &filter.extensions {
            let pattern = format!("*.{ext}");
            let pat = CString::new(pattern.as_str()).expect("Can't convert to CString");
            unsafe { gtk_file_filter_add_pattern(f, pat.as_ptr()) };
        }
        unsafe { gtk_file_chooser_add_filter(chooser, f) };
    }
}
