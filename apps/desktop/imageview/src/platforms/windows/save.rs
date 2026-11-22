/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The controls of the Save As dialog: the file types of the dialog choose the format, and combo
//! boxes below them the JPEG quality and the size vectors are drawn at.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};

use bwindow::NativeFileDialog;

use super::headers::{w, wide};
use super::winrt::{
    ComInterface, ComPtr, GUID, HRESULT, IID_IUNKNOWN, IUnknownVtbl, call, check, interface,
};
use crate::save::{SaveRequest, format_of, has_quality, natural_scale, scale_at, scale_titles};

// SAFETY: This is the SDK IID for IFileDialog.
unsafe impl ComInterface for IFileDialog {
    const IID: GUID = GUID::from_u128(0x42f85136_db7e_439c_85f1_e4075d135fc8);
}
// SAFETY: This is the SDK IID for IFileDialogCustomize.
unsafe impl ComInterface for IFileDialogCustomize {
    const IID: GUID = GUID::from_u128(0xe6fdd21a_163f_4975_9c8c_a69f1ba37034);
}
const IID_IFILE_DIALOG_EVENTS: GUID = GUID::from_u128(0x973510db_7d7f_452b_8975_74a85828d354);

// IFileDialogEvents follows IUnknown with the seven file-dialog callbacks.
#[repr(C)]
struct IFileDialogEventsVtbl {
    base: IUnknownVtbl,
    file_ok: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    folder_changing: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
    folder_change: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    selection_change: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    share_violation:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut u32) -> HRESULT,
    type_change: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    overwrite:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut u32) -> HRESULT,
}

#[repr(C)]
struct DialogEvents {
    vtbl: &'static IFileDialogEventsVtbl,
    refs: AtomicU32,
    customize: ComPtr<IFileDialogCustomize>,
    jpeg: Vec<bool>,
}

static EVENTS_VTABLE: IFileDialogEventsVtbl = IFileDialogEventsVtbl {
    base: IUnknownVtbl {
        QueryInterface: events_query,
        AddRef: events_add_ref,
        Release: events_release,
    },
    file_ok: unchanged,
    folder_changing,
    folder_change: unchanged,
    selection_change: unchanged,
    share_violation: default_response,
    type_change: type_changed,
    overwrite: default_response,
};

unsafe extern "system" fn events_query(
    this: *mut c_void,
    iid: *const GUID,
    result: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: COM supplies valid interface and output pointers.
    unsafe {
        *result = null_mut();
        if *iid == IID_IUNKNOWN || *iid == IID_IFILE_DIALOG_EVENTS {
            events_add_ref(this);
            *result = this;
            0
        } else {
            0x8000_4002u32 as i32
        }
    }
}

unsafe extern "system" fn events_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: COM calls this with a live DialogEvents reference.
    unsafe { &*this.cast::<DialogEvents>() }
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn events_release(this: *mut c_void) -> u32 {
    // SAFETY: Each owned COM reference is released once.
    unsafe {
        let refs = (*this.cast::<DialogEvents>())
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            - 1;
        if refs == 0 {
            drop(Box::from_raw(this.cast::<DialogEvents>()));
        }
        refs
    }
}

const unsafe extern "system" fn unchanged(_: *mut c_void, _: *mut c_void) -> HRESULT {
    0
}
const unsafe extern "system" fn folder_changing(
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
) -> HRESULT {
    0
}
unsafe extern "system" fn default_response(
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    response: *mut u32,
) -> HRESULT {
    // SAFETY: The dialog provides an output for the default shell response.
    unsafe { *response = 0 };
    0
}

unsafe extern "system" fn type_changed(this: *mut c_void, dialog: *mut c_void) -> HRESULT {
    // SAFETY: The shell borrows both the event handler and IFileDialog during this callback.
    unsafe {
        let state = &*this.cast::<DialogEvents>();
        let Some(dialog) = ComPtr::from_raw_borrowed(&dialog) else {
            return 0;
        };
        let mut index = 0u32;
        if call!(dialog, IFileDialog::GetFileTypeIndex, &mut index) >= 0 {
            let visible = index
                .checked_sub(1)
                .and_then(|index| state.jpeg.get(index as usize))
                .copied()
                .unwrap_or(false);
            // ENABLED | VISIBLE is 3 and INACTIVE is 0.
            for id in [QUALITY_GROUP, QUALITY] {
                call!(
                    state.customize,
                    IFileDialogCustomize::SetControlState,
                    id,
                    if visible { 3u32 } else { 0u32 }
                );
            }
        }
    }
    0
}

// The ids of the controls and of their visual groups.
const QUALITY: u32 = 1;
const SIZE: u32 = 2;
const QUALITY_GROUP: u32 = 3;
const SIZE_GROUP: u32 = 4;

// The JPEG qualities to choose from, a native dialog has no slider.
const QUALITIES: [u8; 10] = [100, 95, 90, 85, 80, 75, 70, 60, 50, 40];

/// `COMDLG_FILTERSPEC`
#[repr(C)]
pub(super) struct FilterSpec {
    name: *const u16,
    spec: *const u16,
}

/// The controls of a file dialog.
pub(crate) struct SaveOptions {
    customize: ComPtr<IFileDialogCustomize>,
    dialog: ComPtr<IFileDialog>,
    cookie: Option<u32>,
    // The quality the dialog started with, kept without a quality to choose.
    quality: u8,
    sized: bool,
}

impl SaveOptions {
    /// Adds the file types and controls to the save dialog of a dialog.
    pub(crate) fn new(native: NativeFileDialog, request: &SaveRequest<'_>) -> Option<Self> {
        let NativeFileDialog::Win32(dialog) = native else {
            return None;
        };
        // SAFETY: bwindow passes a live IFileSaveDialog and keeps its reference.
        let dialog = unsafe { ComPtr::from_raw_borrowed(&dialog) }?;
        let file_dialog = dialog.query::<IFileDialog>().ok()?;
        let customize = dialog.query::<IFileDialogCustomize>().ok()?;
        let names: Vec<(Vec<u16>, Vec<u16>)> = request
            .formats
            .iter()
            .map(|(name, extension)| (wide(name), wide(&format!("*.{extension}"))))
            .collect();
        let specs: Vec<FilterSpec> = names
            .iter()
            .map(|(name, spec)| FilterSpec {
                name: name.as_ptr(),
                spec: spec.as_ptr(),
            })
            .collect();
        let selected = format_of(request.name, request.formats);
        let extension = wide(
            request
                .formats
                .get(selected)
                .map_or("", |(_, extension)| extension),
        );
        let quality = QUALITIES
            .iter()
            .enumerate()
            .min_by_key(|(_, quality)| quality.abs_diff(request.quality))
            .map_or(0, |(index, _)| index);
        // SAFETY: Both dialog interfaces are owned, and the array and its strings live
        // through the calls that copy them. The dialog uses 1-based file type indices.
        // Selecting a type also updates the filename extension.
        unsafe {
            call!(
                file_dialog,
                IFileDialog::SetFileTypes,
                specs.len() as u32,
                specs.as_ptr()
            );
            call!(
                file_dialog,
                IFileDialog::SetFileTypeIndex,
                selected as u32 + 1
            );
            call!(
                file_dialog,
                IFileDialog::SetDefaultExtension,
                extension.as_ptr()
            );
            let combo = |id: u32, group: u32, label: &[u16], items: &[String], selected: usize| {
                call!(
                    customize,
                    IFileDialogCustomize::StartVisualGroup,
                    group,
                    label.as_ptr()
                );
                call!(customize, IFileDialogCustomize::AddComboBox, id);
                for (index, item) in items.iter().enumerate() {
                    let item = wide(item);
                    call!(
                        customize,
                        IFileDialogCustomize::AddControlItem,
                        id,
                        index as u32,
                        item.as_ptr()
                    );
                }
                call!(customize, IFileDialogCustomize::EndVisualGroup);
                call!(
                    customize,
                    IFileDialogCustomize::SetSelectedControlItem,
                    id,
                    selected as u32
                );
            };
            let qualities: Vec<String> = QUALITIES
                .iter()
                .map(|quality| format!("{quality}%"))
                .collect();
            // Without a JPEG format to choose there is no quality to choose.
            if request
                .formats
                .iter()
                .any(|(_, extension)| has_quality(extension))
            {
                combo(
                    QUALITY,
                    QUALITY_GROUP,
                    w!("JPEG quality:"),
                    &qualities,
                    quality,
                );
            }
            if let Some(size) = request.size {
                combo(
                    SIZE,
                    SIZE_GROUP,
                    w!("Size:"),
                    &scale_titles(size),
                    natural_scale(),
                );
            }
        }
        let mut cookie = None;
        if request
            .formats
            .iter()
            .any(|(_, extension)| has_quality(extension))
        {
            let events = Box::new(DialogEvents {
                vtbl: &EVENTS_VTABLE,
                refs: AtomicU32::new(1),
                customize: customize.clone(),
                jpeg: request
                    .formats
                    .iter()
                    .map(|(_, extension)| has_quality(extension))
                    .collect(),
            });
            // SAFETY: This is an owned IFileDialogEvents reference with the vtable above.
            let events = unsafe { ComPtr::from_interface(Box::into_raw(events)) }?;
            let mut token = 0;
            // SAFETY: Both objects are live during the initial update, and Advise retains the handler.
            unsafe {
                type_changed(events.as_raw(), file_dialog.as_raw());
                check(call!(
                    file_dialog,
                    IFileDialog::Advise,
                    events.as_raw(),
                    &mut token
                ))
                .ok()?;
            }
            cookie = Some(token);
        }
        Some(Self {
            customize,
            dialog: file_dialog,
            cookie,
            quality: request.quality,
            sized: request.size.is_some(),
        })
    }

    // The chosen item of a combo box.
    fn selected(&self, id: u32) -> Option<usize> {
        let mut item = 0u32;
        // SAFETY: The dialog owns IFileDialogCustomize and item is writable.
        let result = unsafe {
            call!(
                self.customize,
                IFileDialogCustomize::GetSelectedControlItem,
                id,
                &mut item
            )
        };
        (result >= 0).then_some(item as usize)
    }

    /// The chosen JPEG quality.
    pub(crate) fn quality(&self) -> u8 {
        self.selected(QUALITY)
            .and_then(|index| QUALITIES.get(index))
            .copied()
            .unwrap_or(self.quality)
    }

    /// The chosen scale of a vector.
    pub(crate) fn scale(&self) -> f64 {
        if !self.sized {
            return 1.0;
        }
        self.selected(SIZE).map_or(1.0, scale_at)
    }
}

impl Drop for SaveOptions {
    fn drop(&mut self) {
        if let Some(cookie) = self.cookie.take() {
            // SAFETY: Unsubscribe while our dialog reference is still alive.
            unsafe { call!(self.dialog, IFileDialog::Unadvise, cookie) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::winrt::out;
    use super::*;

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> HRESULT;
        fn CoUninitialize();
        fn CoCreateInstance(
            class: *const GUID,
            outer: *mut c_void,
            context: u32,
            iid: *const GUID,
            result: *mut *mut c_void,
        ) -> HRESULT;
    }

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: The test initialized this thread's COM apartment successfully.
            unsafe { CoUninitialize() };
        }
    }

    #[test]
    fn jpeg_quality_tracks_the_native_file_type_and_keeps_its_value() {
        // SAFETY: This test owns its STA and all COM references drop before the apartment.
        check(unsafe { CoInitializeEx(null_mut(), 2) }).expect("COM apartment");
        let _apartment = Apartment;
        let class = GUID::from_u128(0xc0b4e2f3_ba21_4773_8dba_335ec946eb8b);
        let iid = GUID::from_u128(0x84bccd23_5fde_4cdb_aea4_af64b83d78ab);
        // SAFETY: CLSID_FileSaveDialog implements IFileSaveDialog.
        let dialog = out(|raw| unsafe { CoCreateInstance(&class, null_mut(), 1, &iid, raw) })
            .expect("save dialog");
        let request = SaveRequest {
            directory: None,
            name: "image.png",
            quality: 90,
            formats: &[("PNG", "png"), ("JPEG", "jpg"), ("BMP", "bmp")],
            size: None,
        };
        let options = SaveOptions::new(NativeFileDialog::Win32(dialog.as_raw()), &request)
            .expect("custom controls");
        let mut events = DialogEvents {
            vtbl: &EVENTS_VTABLE,
            refs: AtomicU32::new(1),
            customize: options.customize.clone(),
            jpeg: vec![false, true, false],
        };
        // SAFETY: The real shell dialog and its customize interface stay alive throughout.
        unsafe {
            let mut state = u32::MAX;
            check(call!(
                options.customize,
                IFileDialogCustomize::GetControlState,
                QUALITY,
                &mut state
            ))
            .unwrap();
            assert_eq!(state, 0, "PNG must initially hide quality");
            call!(
                options.customize,
                IFileDialogCustomize::SetSelectedControlItem,
                QUALITY,
                3u32
            );
            for (index, expected) in [(2u32, 3u32), (3, 0), (1, 0), (2, 3)] {
                check(call!(options.dialog, IFileDialog::SetFileTypeIndex, index)).unwrap();
                // SetFileTypeIndex outside Show does not raise OnTypeChange; deliver the
                // shell callback directly, exercising its real dialog and control methods.
                type_changed((&raw mut events).cast(), options.dialog.as_raw());
                for id in [QUALITY_GROUP, QUALITY] {
                    check(call!(
                        options.customize,
                        IFileDialogCustomize::GetControlState,
                        id,
                        &mut state
                    ))
                    .unwrap();
                    assert_eq!(state, expected);
                }
                assert_eq!(options.quality(), 85);
            }
        }
    }
}

interface! {
    IFileDialog, IFileDialogVtbl: IModalWindowVtbl {
        SetFileTypes: unsafe extern "system" fn(*mut IFileDialog, u32, *const FilterSpec) -> HRESULT,
        SetFileTypeIndex: unsafe extern "system" fn(*mut IFileDialog, u32) -> HRESULT,
        GetFileTypeIndex: unsafe extern "system" fn(*mut IFileDialog, *mut u32) -> HRESULT,
        Advise: unsafe extern "system" fn(*mut IFileDialog, *mut c_void, *mut u32) -> HRESULT,
        Unadvise: unsafe extern "system" fn(*mut IFileDialog, u32) -> HRESULT,
        _unused_9_21: [*const c_void; 13],
        SetDefaultExtension: unsafe extern "system" fn(*mut IFileDialog, *const u16) -> HRESULT,
    }
}

interface! {
    IFileDialogCustomize, IFileDialogCustomizeVtbl: IUnknownVtbl {
        _unused_3_5: [*const c_void; 3],
        AddComboBox: unsafe extern "system" fn(*mut IFileDialogCustomize, u32) -> HRESULT,
        _unused_7_12: [*const c_void; 6],
        GetControlState: unsafe extern "system" fn(*mut IFileDialogCustomize, u32, *mut u32) -> HRESULT,
        SetControlState: unsafe extern "system" fn(*mut IFileDialogCustomize, u32, u32) -> HRESULT,
        _unused_15_18: [*const c_void; 4],
        AddControlItem: unsafe extern "system" fn(*mut IFileDialogCustomize, u32, u32, *const u16) -> HRESULT,
        _unused_20_23: [*const c_void; 4],
        GetSelectedControlItem: unsafe extern "system" fn(*mut IFileDialogCustomize, u32, *mut u32) -> HRESULT,
        SetSelectedControlItem: unsafe extern "system" fn(*mut IFileDialogCustomize, u32, u32) -> HRESULT,
        StartVisualGroup: unsafe extern "system" fn(*mut IFileDialogCustomize, u32, *const u16) -> HRESULT,
        EndVisualGroup: unsafe extern "system" fn(*mut IFileDialogCustomize) -> HRESULT,
    }
}

#[repr(C)]
struct IModalWindowVtbl {
    base: IUnknownVtbl,
    _unused_3: *const c_void,
}
