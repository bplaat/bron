/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering, fence};

use bwindow::WindowEvent;

use super::com::ComPtr;
use super::headers::*;

/// The COM drop target registered on each browser child window
#[repr(C)]
pub(super) struct FileDropTarget {
    interface: IDropTarget,
    sender: bwindow::WindowEventSender,
    refs: AtomicU32,
    hwnd: HWND,
    valid: Cell<bool>,
}

static FILE_DROP_TARGET_VTBL: IDropTargetVtbl = IDropTargetVtbl {
    base: IUnknownVtbl {
        QueryInterface: file_drop_query_interface,
        AddRef: file_drop_add_ref,
        Release: file_drop_release,
    },
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

pub(super) fn revoke_file_drop_targets(targets: &RefCell<Vec<ComPtr<FileDropTarget>>>) {
    let targets = mem::take(&mut *targets.borrow_mut());
    for target in &targets {
        unsafe { RevokeDragDrop(target.hwnd) };
    }
}

/// Registers a drop target on every browser child window
pub(super) unsafe fn install_file_drop_targets(
    hwnd: HWND,
    sender: bwindow::WindowEventSender,
    targets: &RefCell<Vec<ComPtr<FileDropTarget>>>,
) {
    struct State<'a> {
        sender: bwindow::WindowEventSender,
        targets: &'a RefCell<Vec<ComPtr<FileDropTarget>>>,
    }
    let state = State { sender, targets };
    unsafe extern "system" fn enum_child(hwnd: HWND, data: LPARAM) -> BOOL {
        let state = unsafe { &*(data as *const State<'_>) };
        let target = Box::into_raw(Box::new(FileDropTarget {
            sender: state.sender.clone(),
            interface: IDropTarget {
                vtbl: &FILE_DROP_TARGET_VTBL,
            },
            refs: AtomicU32::new(1),
            hwnd,
            valid: Cell::new(false),
        }));
        let target =
            unsafe { ComPtr::from_raw(target) }.expect("drop target allocation is non-null");
        let revoked = unsafe { RevokeDragDrop(hwnd) };
        if revoked != DRAGDROP_E_INVALIDHWND
            && unsafe { RegisterDragDrop(hwnd, target.as_ptr().cast()) } == S_OK
        {
            state.targets.borrow_mut().push(target);
        }
        TRUE
    }

    unsafe { EnumChildWindows(hwnd, enum_child, &state as *const State<'_> as LPARAM) };
}

unsafe extern "system" fn file_drop_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if object.is_null() {
        return E_POINTER;
    }
    if !iid.is_null() && (unsafe { *iid == IID_IUNKNOWN } || unsafe { *iid == IID_IDROP_TARGET }) {
        unsafe {
            *object = this;
            file_drop_add_ref(this);
        }
        S_OK
    } else {
        unsafe { *object = null_mut() };
        E_NOINTERFACE
    }
}

unsafe extern "system" fn file_drop_add_ref(this: *mut c_void) -> u32 {
    let target = unsafe { &*this.cast::<FileDropTarget>() };
    target.refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn file_drop_release(this: *mut c_void) -> u32 {
    let target = unsafe { &*this.cast::<FileDropTarget>() };
    let remaining = target.refs.fetch_sub(1, Ordering::Release) - 1;
    if remaining == 0 {
        fence(Ordering::Acquire);
        unsafe { drop(Box::from_raw(this.cast::<FileDropTarget>())) };
    }
    remaining
}

unsafe extern "system" fn file_drop_drag_enter(
    this: *mut IDropTarget,
    data: *mut IDataObject,
    _key_state: u32,
    _point: POINT,
    effect: *mut u32,
) -> HRESULT {
    let target = unsafe { &*this.cast::<FileDropTarget>() };
    target.valid.set(unsafe { file_drop_paths(data) }.is_some());
    unsafe {
        *effect = if target.valid.get() {
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
    let target = unsafe { &*this.cast::<FileDropTarget>() };
    unsafe {
        *effect = if target.valid.get() {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        }
    };
    S_OK
}

unsafe extern "system" fn file_drop_drag_leave(this: *mut IDropTarget) -> HRESULT {
    unsafe { (*this.cast::<FileDropTarget>()).valid.set(false) };
    S_OK
}

unsafe extern "system" fn file_drop(
    this: *mut IDropTarget,
    data: *mut IDataObject,
    _key_state: u32,
    _point: POINT,
    effect: *mut u32,
) -> HRESULT {
    let target = unsafe { &*this.cast::<FileDropTarget>() };
    if target.valid.get()
        && let Some(paths) = unsafe { file_drop_paths(data) }
    {
        for path in paths {
            target.sender.send(WindowEvent::DroppedFile(path));
        }
        unsafe { *effect = DROPEFFECT_COPY };
    } else {
        unsafe { *effect = DROPEFFECT_NONE };
    }
    target.valid.set(false);
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
    if unsafe { ((*(*data).vtbl).GetData)(data, &mut format, &mut medium) } != S_OK {
        return None;
    }

    let drop = medium.data as HDROP;
    let count = unsafe { DragQueryFileW(drop, u32::MAX, null_mut(), 0) };
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let length = unsafe { DragQueryFileW(drop, index, null_mut(), 0) };
        let mut buffer = vec![0; length as usize + 1];
        unsafe { DragQueryFileW(drop, index, buffer.as_mut_ptr(), buffer.len() as u32) };
        paths.push(String::from_utf16_lossy(&buffer[..length as usize]).into());
    }
    unsafe { ReleaseStgMedium(&mut medium) };
    Some(paths)
}
