/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(any(target_vendor = "winxp", test))]
use std::ffi::OsString;
use std::ffi::c_void;
use std::mem::size_of;
#[cfg(any(target_vendor = "winxp", test))]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use super::com::ComPtr;
use super::headers::*;

const FIRST_CUSTOM_BUTTON_ID: i32 = 1000;

pub(crate) struct PlatformMessageDialog;

impl crate::MessageDialogInterface for PlatformMessageDialog {
    fn show(dialog: crate::MessageDialog<'_>) -> crate::MessageDialogResult {
        let parent = dialog
            .parent
            .map(|window| window.0.hwnd)
            .unwrap_or_else(|| unsafe { GetActiveWindow() });
        let Some(task_dialog) = optional_proc!(
            7,
            "comctl32",
            TaskDialogIndirect(*const TASKDIALOGCONFIG, *mut i32, *mut i32, *mut BOOL) -> HRESULT
        ) else {
            return fallback_message_box(parent, &dialog);
        };
        let title = wide(&dialog.title);
        let description = wide(&dialog.description);
        let (common_buttons, labels) = task_dialog_buttons(&dialog.buttons);
        let label_storage = labels
            .iter()
            .map(|label| wide(label).into_boxed_slice())
            .collect::<Vec<_>>();
        let custom_buttons = label_storage
            .iter()
            .enumerate()
            .map(|(index, label)| TASKDIALOG_BUTTON {
                nButtonID: FIRST_CUSTOM_BUTTON_ID + index as i32,
                pszButtonText: label.as_ptr(),
            })
            .collect::<Vec<_>>();
        let main_icon = match dialog.level {
            crate::MessageLevel::Info => TD_INFORMATION_ICON,
            crate::MessageLevel::Warning => TD_WARNING_ICON,
            crate::MessageLevel::Error => TD_ERROR_ICON,
        };
        let config = TASKDIALOGCONFIG {
            cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: parent,
            hInstance: null(),
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
            dwCommonButtons: common_buttons,
            pszWindowTitle: title.as_ptr(),
            mainIcon: main_icon,
            pszMainInstruction: null(),
            pszContent: description.as_ptr(),
            cButtons: custom_buttons.len() as u32,
            pButtons: if custom_buttons.is_empty() {
                null()
            } else {
                custom_buttons.as_ptr()
            },
            nDefaultButton: if custom_buttons.is_empty() {
                0
            } else {
                FIRST_CUSTOM_BUTTON_ID
            },
            cRadioButtons: 0,
            pRadioButtons: null(),
            nDefaultRadioButton: 0,
            pszVerificationText: null(),
            pszExpandedInformation: null(),
            pszExpandedControlText: null(),
            pszCollapsedControlText: null(),
            footerIcon: null(),
            pszFooter: null(),
            pfCallback: None,
            lpCallbackData: 0,
            cxWidth: 0,
        };
        let mut selected = 0;
        let result = unsafe { task_dialog(&config, &mut selected, null_mut(), null_mut()) };
        if result != S_OK {
            return fallback_message_box(parent, &dialog);
        }
        message_dialog_result(&dialog.buttons, selected)
    }
}

fn task_dialog_buttons(buttons: &crate::MessageButtons) -> (u32, Vec<&str>) {
    match buttons {
        crate::MessageButtons::Ok => (TDCBF_OK_BUTTON, Vec::new()),
        crate::MessageButtons::OkCancel => (TDCBF_OK_BUTTON | TDCBF_CANCEL_BUTTON, Vec::new()),
        crate::MessageButtons::YesNo => (TDCBF_YES_BUTTON | TDCBF_NO_BUTTON, Vec::new()),
        crate::MessageButtons::YesNoCancel => (
            TDCBF_YES_BUTTON | TDCBF_NO_BUTTON | TDCBF_CANCEL_BUTTON,
            Vec::new(),
        ),
        crate::MessageButtons::OkCustom(ok) => (0, vec![ok]),
        crate::MessageButtons::OkCancelCustom(ok, cancel) => (0, vec![ok, cancel]),
        crate::MessageButtons::YesNoCancelCustom(yes, no, cancel) => (0, vec![yes, no, cancel]),
    }
}

fn message_dialog_result(
    buttons: &crate::MessageButtons,
    selected: i32,
) -> crate::MessageDialogResult {
    use crate::MessageDialogResult as Result;
    match selected {
        IDOK => Result::Ok,
        IDYES => Result::Yes,
        IDNO => Result::No,
        id if id >= FIRST_CUSTOM_BUTTON_ID => {
            let index = (id - FIRST_CUSTOM_BUTTON_ID) as usize;
            crate::dialog::message_dialog_result(buttons, index)
        }
        _ => Result::Cancel,
    }
}

fn fallback_message_box(
    parent: HWND,
    dialog: &crate::MessageDialog<'_>,
) -> crate::MessageDialogResult {
    let style = match &dialog.buttons {
        crate::MessageButtons::Ok | crate::MessageButtons::OkCustom(_) => MB_OK,
        crate::MessageButtons::OkCancel | crate::MessageButtons::OkCancelCustom(_, _) => {
            MB_OKCANCEL
        }
        crate::MessageButtons::YesNo => MB_YESNO,
        _ => MB_YESNOCANCEL,
    } | match dialog.level {
        crate::MessageLevel::Info => MB_ICONINFORMATION,
        crate::MessageLevel::Warning => MB_ICONWARNING,
        crate::MessageLevel::Error => MB_ICONERROR,
    };
    let selected = unsafe {
        MessageBoxW(
            parent,
            wide(&dialog.description).as_ptr(),
            wide(&dialog.title).as_ptr(),
            style,
        )
    };
    match (&dialog.buttons, selected) {
        (
            crate::MessageButtons::OkCustom(ok) | crate::MessageButtons::OkCancelCustom(ok, _),
            IDOK,
        ) => crate::MessageDialogResult::Custom(ok.clone()),
        (crate::MessageButtons::YesNoCancelCustom(yes, _, _), IDYES) => {
            crate::MessageDialogResult::Custom(yes.clone())
        }
        (crate::MessageButtons::YesNoCancelCustom(_, no, _), IDNO) => {
            crate::MessageDialogResult::Custom(no.clone())
        }
        (crate::MessageButtons::OkCancelCustom(_, cancel), _)
        | (crate::MessageButtons::YesNoCancelCustom(_, _, cancel), IDCANCEL) => {
            crate::MessageDialogResult::Custom(cancel.clone())
        }
        (_, selected) => message_dialog_result(&dialog.buttons, selected),
    }
}

pub(crate) struct PlatformFileDialog;

impl crate::FileDialogInterface for PlatformFileDialog {
    fn pick_file(dialog: crate::FileDialog<'_>) -> Option<PathBuf> {
        unsafe {
            let Ok(dlg) =
                create_dialog::<IFileOpenDialog>(&CLSID_FileOpenDialog, &IID_IFileOpenDialog)
            else {
                #[cfg(target_vendor = "winxp")]
                return fallback_file_dialog(dialog, false, false)?
                    .into_iter()
                    .next();
                #[cfg(not(target_vendor = "winxp"))]
                return None;
            };
            dlg.SetOptions(FOS_NOCHANGEDIR | FOS_PATHMUSTEXIST | FOS_FILEMUSTEXIST);
            let title = dialog.title.as_deref().map(wide);
            if let Some(ref t) = title {
                dlg.SetTitle(t.as_ptr());
            }
            let dir_item = make_shell_item_from_path(dialog.directory.as_ref());
            if let Some(item) = dir_item {
                dlg.SetFolder(item.as_ptr());
            }
            let (specs, _storage) = build_com_filters(&dialog.filters);
            if !specs.is_empty() {
                dlg.SetFileTypes(&specs);
            }
            let hwnd = dialog
                .parent
                .map(|window| window.0.hwnd)
                .unwrap_or_else(|| GetActiveWindow());
            if dlg.Show(hwnd) != S_OK {
                return None;
            }
            let item = dlg.GetResult().ok()?;
            shell_item_path(&item)
        }
    }

    fn pick_files(dialog: crate::FileDialog<'_>) -> Option<Vec<PathBuf>> {
        unsafe {
            let Ok(dlg) =
                create_dialog::<IFileOpenDialog>(&CLSID_FileOpenDialog, &IID_IFileOpenDialog)
            else {
                #[cfg(target_vendor = "winxp")]
                return fallback_file_dialog(dialog, false, true);
                #[cfg(not(target_vendor = "winxp"))]
                return None;
            };
            dlg.SetOptions(
                FOS_NOCHANGEDIR | FOS_PATHMUSTEXIST | FOS_FILEMUSTEXIST | FOS_ALLOWMULTISELECT,
            );
            let title = dialog.title.as_deref().map(wide);
            if let Some(ref t) = title {
                dlg.SetTitle(t.as_ptr());
            }
            let dir_item = make_shell_item_from_path(dialog.directory.as_ref());
            if let Some(item) = dir_item {
                dlg.SetFolder(item.as_ptr());
            }
            let (specs, _storage) = build_com_filters(&dialog.filters);
            if !specs.is_empty() {
                dlg.SetFileTypes(&specs);
            }
            let hwnd = dialog
                .parent
                .map(|window| window.0.hwnd)
                .unwrap_or_else(|| GetActiveWindow());
            if dlg.Show(hwnd) != S_OK {
                return None;
            }
            let items = dlg.GetResults().ok()?;
            let mut count = 0;
            items.GetCount(&mut count);
            let mut paths = Vec::with_capacity(count as usize);
            for index in 0..count {
                if let Ok(item) = items.GetItemAt(index)
                    && let Some(path) = shell_item_path(&item)
                {
                    paths.push(path);
                }
            }
            (!paths.is_empty()).then_some(paths)
        }
    }

    fn save_file(mut dialog: crate::FileDialog<'_>) -> Option<PathBuf> {
        unsafe {
            let Ok(dlg) =
                create_dialog::<IFileSaveDialog>(&CLSID_FileSaveDialog, &IID_IFileSaveDialog)
            else {
                #[cfg(target_vendor = "winxp")]
                return fallback_file_dialog(dialog, true, false)?
                    .into_iter()
                    .next();
                #[cfg(not(target_vendor = "winxp"))]
                return None;
            };
            dlg.SetOptions(FOS_NOCHANGEDIR | FOS_OVERWRITEPROMPT);
            let title = dialog.title.as_deref().map(wide);
            if let Some(ref t) = title {
                dlg.SetTitle(t.as_ptr());
            }
            let dir_item = make_shell_item_from_path(dialog.directory.as_ref());
            if let Some(item) = dir_item {
                dlg.SetFolder(item.as_ptr());
            }
            let filename = dialog.filename.as_deref().map(wide);
            if let Some(ref f) = filename {
                dlg.SetFileName(f.as_ptr());
            }
            let (specs, _storage) = build_com_filters(&dialog.filters);
            if !specs.is_empty() {
                dlg.SetFileTypes(&specs);
            }
            let default_extension = dialog
                .filters
                .first()
                .and_then(|f| f.extensions.first())
                .map(|e| wide(e));
            if let Some(ref e) = default_extension {
                dlg.SetDefaultExtension(e.as_ptr());
            }
            if let Some(customize) = dialog.customize.take() {
                customize(crate::NativeFileDialog::Win32(dlg.as_ptr().cast()));
            }
            let hwnd = dialog
                .parent
                .map(|window| window.0.hwnd)
                .unwrap_or_else(|| GetActiveWindow());
            if dlg.Show(hwnd) != S_OK {
                return None;
            }
            let item = dlg.GetResult().ok()?;
            shell_item_path(&item)
        }
    }
}

type FilterStorage = Vec<(Box<[u16]>, Box<[u16]>)>;

fn build_com_filters(
    filters: &[crate::FileDialogFilter],
) -> (Vec<COMDLG_FILTERSPEC>, FilterStorage) {
    let mut storage: FilterStorage = Vec::new();
    let mut specs: Vec<COMDLG_FILTERSPEC> = Vec::new();
    for f in filters {
        let name: Box<[u16]> = wide(&f.name).into_boxed_slice();
        let pattern = f
            .extensions
            .iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(";");
        let spec: Box<[u16]> = wide(&pattern).into_boxed_slice();
        specs.push(COMDLG_FILTERSPEC {
            pszName: name.as_ptr(),
            pszSpec: spec.as_ptr(),
        });
        storage.push((name, spec));
    }
    (specs, storage)
}

unsafe fn create_dialog<T>(class: &GUID, iid: &GUID) -> Result<ComPtr<T>, HRESULT> {
    unsafe {
        ComPtr::create(|out: *mut *mut T| {
            CoCreateInstance(class, null_mut(), CLSCTX_INPROC_SERVER, iid, out.cast())
        })
    }
}

unsafe fn make_shell_item_from_path(path: Option<&PathBuf>) -> Option<ComPtr<IShellItem>> {
    let path = path?;
    let create_item = optional_proc!(
        7,
        "shell32",
        SHCreateItemFromParsingName(
            *const u16,
            *mut c_void,
            *const GUID,
            *mut *mut c_void,
        ) -> HRESULT
    )?;
    let w = wide(&path.to_string_lossy());
    unsafe {
        ComPtr::create(|out: *mut *mut IShellItem| {
            create_item(w.as_ptr(), null_mut(), &IID_IShellItem, out.cast())
        })
        .ok()
    }
}

unsafe fn shell_item_path(item: &IShellItem) -> Option<PathBuf> {
    let mut name = LPWSTR::default();
    if unsafe { item.GetDisplayName(SIGDN_FILESYSPATH, name.as_mut_ptr()) } == S_OK {
        Some(PathBuf::from(name.to_string()))
    } else {
        None
    }
}

#[cfg(target_vendor = "winxp")]
fn fallback_file_dialog(
    dialog: crate::FileDialog<'_>,
    save: bool,
    multiple: bool,
) -> Option<Vec<PathBuf>> {
    let mut buffer = vec![0u16; 32_768];
    if let Some(filename) = &dialog.filename {
        let filename = wide(filename);
        if filename.len() > buffer.len() {
            return None;
        }
        buffer[..filename.len()].copy_from_slice(&filename);
    }
    let (_, filters) = build_com_filters(&dialog.filters);
    let filter = filters
        .iter()
        .flat_map(|(name, pattern)| name.iter().chain(pattern.iter()))
        .copied()
        .chain([0])
        .collect::<Vec<_>>();
    let title = dialog.title.as_deref().map(wide);
    let directory = dialog.directory.as_ref().map(|path| {
        path.as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>()
    });
    let extension = dialog
        .filters
        .first()
        .and_then(|filter| filter.extensions.first())
        .filter(|_| save)
        .map(|extension| wide(extension));
    // All fields allow zero/null defaults. The backing buffers live through the modal call.
    let mut config = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: dialog
            .parent
            .map(|window| window.0.hwnd)
            .unwrap_or_else(|| unsafe { GetActiveWindow() }),
        lpstrFilter: if filters.is_empty() {
            null()
        } else {
            filter.as_ptr()
        },
        nFilterIndex: 1,
        lpstrFile: buffer.as_mut_ptr(),
        nMaxFile: buffer.len() as u32,
        lpstrInitialDir: directory.as_ref().map_or(null(), |value| value.as_ptr()),
        lpstrTitle: title.as_ref().map_or(null(), |value| value.as_ptr()),
        lpstrDefExt: extension.as_ref().map_or(null(), |value| value.as_ptr()),
        Flags: OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            }
            | if multiple { OFN_ALLOWMULTISELECT } else { 0 },
        ..unsafe { std::mem::zeroed() }
    };
    let success = unsafe {
        if save {
            GetSaveFileNameW(&mut config)
        } else {
            GetOpenFileNameW(&mut config)
        }
    };
    if success == 0 {
        return None;
    }
    selected_paths(&buffer)
}

#[cfg(any(target_vendor = "winxp", test))]
fn selected_paths(buffer: &[u16]) -> Option<Vec<PathBuf>> {
    let mut parts = buffer
        .split(|&unit| unit == 0)
        .take_while(|part| !part.is_empty());
    let first = PathBuf::from(OsString::from_wide(parts.next()?));
    let mut paths = parts
        .map(|name| first.join(OsString::from_wide(name)))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        paths.push(first);
    }
    Some(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_dialog_layout_matches_windows_headers() {
        assert_eq!(
            size_of::<OPENFILENAMEW>(),
            if cfg!(target_pointer_width = "32") {
                88
            } else {
                152
            }
        );
    }

    #[test]
    fn reads_single_and_multiple_selections() {
        assert_eq!(
            selected_paths(&wide("C:\\My Files\\one.txt")),
            Some(vec![PathBuf::from("C:\\My Files\\one.txt")])
        );
        let buffer = wide("C:\\My Files\0one.txt\0two words.txt\0");
        assert_eq!(
            selected_paths(&buffer),
            Some(vec![
                PathBuf::from("C:\\My Files\\one.txt"),
                PathBuf::from("C:\\My Files\\two words.txt")
            ])
        );
        assert_eq!(selected_paths(&[0, 0]), None);
    }

    #[test]
    fn preserves_non_unicode_windows_paths() {
        let buffer = [b'C' as u16, b':' as u16, b'\\' as u16, 0xd800, 0, 0];
        let paths = selected_paths(&buffer).unwrap();
        assert_eq!(
            paths[0].as_os_str().encode_wide().collect::<Vec<_>>(),
            buffer[..4]
        );
    }

    #[test]
    fn task_dialog_layout_matches_windows_headers() {
        assert_eq!(size_of::<TASKDIALOG_BUTTON>(), 4 + size_of::<*const u16>());
        assert_eq!(
            size_of::<TASKDIALOGCONFIG>(),
            if cfg!(target_pointer_width = "32") {
                96
            } else {
                160
            }
        );
    }
}
