/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{OsString, c_void};
use std::mem;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};

use bwindow::WindowEvent;

use super::headers::*;
use super::webview::WebviewData;

/// The COM drop target registered on each WebView2 child window
#[repr(C)]
pub(super) struct FileDropTarget {
    interface: IDropTarget,
    sender: bwindow::WindowEventSender,
    refs: AtomicU32,
    hwnd: HWND,
    valid: bool,
}

static FILE_DROP_TARGET_VTBL: IDropTargetVtbl = IDropTargetVtbl {
    QueryInterface: file_drop_query_interface,
    AddRef: file_drop_add_ref,
    Release: file_drop_release,
    DragEnter: file_drop_drag_enter,
    DragOver: file_drop_drag_over,
    DragLeave: file_drop_drag_leave,
    Drop: file_drop,
};

const IID_IDROP_TARGET: GUID = GUID {
    data1: 0x00000122,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};

pub(super) fn revoke_file_drop_targets(data: &WebviewData) {
    let targets = mem::take(&mut *data.drop_targets.borrow_mut());
    for target in &targets {
        unsafe { RevokeDragDrop(target.hwnd) };
    }
}

/// Registers a drop target on every WebView2 child window
pub(super) unsafe fn install_file_drop_targets(data: &WebviewData) {
    unsafe extern "system" fn enum_child(hwnd: HWND, data: LPARAM) -> BOOL {
        let data = unsafe { &*(data as *const WebviewData) };
        let mut target = Box::new(FileDropTarget {
            sender: data.attachment.event_sender(),
            interface: IDropTarget {
                lpVtbl: &FILE_DROP_TARGET_VTBL,
            },
            refs: AtomicU32::new(1),
            hwnd,
            valid: false,
        });
        let revoked = unsafe { RevokeDragDrop(hwnd) };
        if revoked != DRAGDROP_E_INVALIDHWND
            && unsafe { RegisterDragDrop(hwnd, &mut target.interface) } == S_OK
        {
            data.drop_targets.borrow_mut().push(target);
        }
        TRUE
    }

    unsafe { EnumChildWindows(data.hwnd, enum_child, data as *const WebviewData as LPARAM) };
}

unsafe extern "system" fn file_drop_query_interface(
    this: *mut IDropTarget,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if !iid.is_null() && (unsafe { *iid == IID_IUNKNOWN } || unsafe { *iid == IID_IDROP_TARGET }) {
        unsafe {
            *object = this as *mut c_void;
            file_drop_add_ref(this);
        }
        S_OK
    } else {
        unsafe { *object = null_mut() };
        E_NOINTERFACE
    }
}

unsafe extern "system" fn file_drop_add_ref(this: *mut IDropTarget) -> u32 {
    let target = unsafe { &*(this as *mut FileDropTarget) };
    target.refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn file_drop_release(this: *mut IDropTarget) -> u32 {
    let target = unsafe { &*(this as *mut FileDropTarget) };
    target.refs.fetch_sub(1, Ordering::Release) - 1
}

unsafe extern "system" fn file_drop_drag_enter(
    this: *mut IDropTarget,
    data: *mut IDataObject,
    _key_state: u32,
    _point: POINT,
    effect: *mut u32,
) -> HRESULT {
    let target = unsafe { &mut *(this as *mut FileDropTarget) };
    target.valid = unsafe { file_drop_paths(data) }.is_some();
    unsafe {
        *effect = if target.valid {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        }
    };
    S_OK
}

unsafe extern "system" fn file_drop_drag_over(
    this: *mut IDropTarget,
    _key_state: u32,
    _point: POINT,
    effect: *mut u32,
) -> HRESULT {
    let target = unsafe { &*(this as *mut FileDropTarget) };
    unsafe {
        *effect = if target.valid {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        }
    };
    S_OK
}

unsafe extern "system" fn file_drop_drag_leave(this: *mut IDropTarget) -> HRESULT {
    unsafe { (*(this as *mut FileDropTarget)).valid = false };
    S_OK
}

unsafe extern "system" fn file_drop(
    this: *mut IDropTarget,
    data: *mut IDataObject,
    _key_state: u32,
    _point: POINT,
    effect: *mut u32,
) -> HRESULT {
    let target = unsafe { &mut *(this as *mut FileDropTarget) };
    if target.valid
        && let Some(paths) = unsafe { file_drop_paths(data) }
    {
        for path in paths {
            target.sender.send(WindowEvent::DroppedFile(path));
        }
        unsafe { *effect = DROPEFFECT_COPY };
    } else {
        unsafe { *effect = DROPEFFECT_NONE };
    }
    target.valid = false;
    S_OK
}

unsafe fn file_drop_paths(data: *mut IDataObject) -> Option<Vec<PathBuf>> {
    if data.is_null() {
        return None;
    }
    let mut format = FORMATETC {
        cfFormat: CF_HDROP,
        ptd: null_mut(),
        dwAspect: DVASPECT_CONTENT,
        lindex: -1,
        tymed: TYMED_HGLOBAL,
    };
    let mut medium: STGMEDIUM = unsafe { mem::zeroed() };
    if unsafe { ((*(*data).lpVtbl).GetData)(data, &mut format, &mut medium) } != S_OK {
        return None;
    }

    let drop = medium.data as HDROP;
    let count = unsafe { DragQueryFileW(drop, u32::MAX, null_mut(), 0) };
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let length = unsafe { DragQueryFileW(drop, index, null_mut(), 0) };
        let mut buffer = vec![0; length as usize + 1];
        unsafe { DragQueryFileW(drop, index, buffer.as_mut_ptr(), buffer.len() as u32) };
        paths.push(OsString::from_wide(&buffer[..length as usize]).into());
    }
    unsafe { ReleaseStgMedium(&mut medium) };
    Some(paths)
}
