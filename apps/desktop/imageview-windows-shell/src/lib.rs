/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Windows Explorer preview and thumbnail handlers for the image formats ImageView displays.

#![allow(unsafe_code)]
// The MSVC linker wants the COM entry points exported as PRIVATE, which rustc can't express.
#![allow(linker_messages)]

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::headers::*;

mod headers;
mod preview;
mod thumbnail;

const CLSID_THUMBNAIL_PROVIDER: GUID = GUID::new(
    0x8098_2b38,
    0xae66,
    0x4d28,
    [0x95, 0x2e, 0x41, 0x0e, 0x18, 0x58, 0x80, 0x1b],
);
const CLSID_PREVIEW_HANDLER: GUID = GUID::new(
    0x1d46_8ad5,
    0xf899,
    0x453b,
    [0x89, 0x15, 0xd1, 0x9c, 0xd8, 0x84, 0xdb, 0xad],
);

// Larger files are not shown, which keeps a thumbnail request from exhausting memory.
const MAX_FILE_SIZE: usize = 256 * 1024 * 1024;

/// The interfaces a COM object implements, in the order of its vtables.
pub(crate) trait Interfaces<const N: usize> {
    const IIDS: [GUID; N];
}

/// A reference counted COM object, whose interface pointers point at its vtables.
#[repr(C)]
pub(crate) struct ComObject<T: Interfaces<N>, const N: usize> {
    vtables: [*const c_void; N],
    references: AtomicU32,
    pub(crate) value: T,
}

impl<T: Interfaces<N>, const N: usize> ComObject<T, N> {
    /// Creates an object with one reference, returning its first interface.
    pub(crate) fn create(vtables: [*const c_void; N], value: T) -> *mut c_void {
        let object = Box::into_raw(Box::new(Self {
            vtables,
            references: AtomicU32::new(1),
            value,
        }));
        // SAFETY: The object was just allocated.
        unsafe { (&raw mut (*object).vtables).cast() }
    }

    /// The object of the interface pointer at `index`.
    ///
    /// # Safety
    /// `this` must be the interface at `index` of a live object.
    pub(crate) unsafe fn from_interface<'a>(this: *mut c_void, index: usize) -> &'a Self {
        // SAFETY: The vtables are the first field, so the object starts `index` pointers before.
        unsafe { &*this.cast::<*const c_void>().sub(index).cast::<Self>() }
    }

    fn query(&self, iid: &GUID, out: *mut *mut c_void) -> HRESULT {
        if out.is_null() {
            return E_POINTER;
        }
        let index = if *iid == IID_IUNKNOWN {
            Some(0)
        } else {
            T::IIDS.iter().position(|interface| interface == iid)
        };
        let Some(index) = index else {
            // SAFETY: The caller supplies a writable out pointer.
            unsafe { *out = null_mut() };
            return E_NOINTERFACE;
        };
        self.references.fetch_add(1, Ordering::Relaxed);
        // SAFETY: The caller supplies a writable out pointer.
        unsafe { *out = (&raw const self.vtables[index]).cast_mut().cast() };
        S_OK
    }
}

/// The IUnknown methods of the interface at index `I`.
pub(crate) const fn unknown<T: Interfaces<N>, const N: usize, const I: usize>() -> IUnknownVtbl {
    unsafe extern "system" fn query_interface<T: Interfaces<N>, const N: usize, const I: usize>(
        this: *mut c_void,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        if iid.is_null() {
            return E_POINTER;
        }
        // SAFETY: COM calls this with the interface at index I and a valid IID.
        unsafe { ComObject::<T, N>::from_interface(this, I).query(&*iid, out) }
    }
    unsafe extern "system" fn add_ref<T: Interfaces<N>, const N: usize, const I: usize>(
        this: *mut c_void,
    ) -> u32 {
        // SAFETY: COM calls this with the interface at index I.
        let object = unsafe { ComObject::<T, N>::from_interface(this, I) };
        object.references.fetch_add(1, Ordering::Relaxed) + 1
    }
    unsafe extern "system" fn release<T: Interfaces<N>, const N: usize, const I: usize>(
        this: *mut c_void,
    ) -> u32 {
        // SAFETY: COM calls this with the interface at index I.
        let object = unsafe { ComObject::<T, N>::from_interface(this, I) };
        let references = object.references.fetch_sub(1, Ordering::AcqRel) - 1;
        if references == 0 {
            // SAFETY: The last reference is gone, so the boxed object is freed once.
            drop(unsafe { Box::from_raw(std::ptr::from_ref(object).cast_mut()) });
        }
        references
    }
    IUnknownVtbl {
        QueryInterface: query_interface::<T, N, I>,
        AddRef: add_ref::<T, N, I>,
        Release: release::<T, N, I>,
    }
}

/// Runs a COM method, a panic must not unwind into the caller.
pub(crate) fn guard(method: impl FnOnce() -> HRESULT) -> HRESULT {
    catch_unwind(AssertUnwindSafe(method)).unwrap_or(E_FAIL)
}

/// Reads a whole stream.
///
/// # Safety
/// `stream` must be a live IStream.
pub(crate) unsafe fn read_stream(stream: *mut c_void) -> Option<Vec<u8>> {
    if stream.is_null() {
        return None;
    }
    // SAFETY: The caller supplies a live stream, whose vtable starts with ISequentialStream.
    let vtable = unsafe { &**stream.cast::<*const IStreamVtbl>() };
    let mut bytes = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let mut read = 0;
        // SAFETY: The chunk is writable for its length.
        let result = unsafe {
            (vtable.Read)(
                stream,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )
        };
        if result < 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
        if bytes.len() > MAX_FILE_SIZE {
            return None;
        }
        if result == S_FALSE || read == 0 {
            return Some(bytes);
        }
    }
}

/// Whether Windows apps use the dark theme.
pub(crate) fn is_dark() -> bool {
    let mut light = 1u32;
    let mut size = size_of::<u32>() as u32;
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let value = wide("AppsUseLightTheme");
    // SAFETY: The strings are null terminated and the data is a DWORD.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut light).cast(),
            &mut size,
        )
    };
    status == 0 && light == 0
}

// MARK: Class factory
#[repr(C)]
#[allow(non_snake_case)]
struct IClassFactoryVtbl {
    base: IUnknownVtbl,
    CreateInstance: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const GUID,
        *mut *mut c_void,
    ) -> HRESULT,
    LockServer: unsafe extern "system" fn(*mut c_void, BOOL) -> HRESULT,
}

/// A process-lifetime class factory that creates objects with one reference.
#[repr(C)]
struct ClassFactory {
    vtable: &'static IClassFactoryVtbl,
    create: fn() -> *mut c_void,
}

// SAFETY: The factories are immutable statics.
unsafe impl Sync for ClassFactory {}

static FACTORY_VTABLE: IClassFactoryVtbl = IClassFactoryVtbl {
    base: IUnknownVtbl {
        QueryInterface: factory_query_interface,
        AddRef: factory_add_ref,
        Release: factory_add_ref,
    },
    CreateInstance: factory_create_instance,
    LockServer: factory_lock_server,
};

static THUMBNAIL_FACTORY: ClassFactory = ClassFactory {
    vtable: &FACTORY_VTABLE,
    create: thumbnail::create,
};

static PREVIEW_FACTORY: ClassFactory = ClassFactory {
    vtable: &FACTORY_VTABLE,
    create: preview::create,
};

unsafe extern "system" fn factory_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if iid.is_null() || out.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM supplies a valid IID and a writable out pointer.
    unsafe {
        if *iid == IID_IUNKNOWN || *iid == IID_ICLASS_FACTORY {
            *out = this;
            S_OK
        } else {
            *out = null_mut();
            E_NOINTERFACE
        }
    }
}

// The factories are statics, so references are not counted.
const unsafe extern "system" fn factory_add_ref(_: *mut c_void) -> u32 {
    1
}

unsafe extern "system" fn factory_create_instance(
    this: *mut c_void,
    outer: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if !outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    guard(|| {
        // SAFETY: COM calls this with one of the static factories.
        let factory = unsafe { &*this.cast::<ClassFactory>() };
        let object = (factory.create)();
        // SAFETY: Every created object starts with an IUnknown vtable, its own reference is
        // released after the requested interface took one.
        unsafe {
            let vtable = &**object.cast::<*const IUnknownVtbl>();
            let result = (vtable.QueryInterface)(object, iid, out);
            (vtable.Release)(object);
            result
        }
    })
}

const unsafe extern "system" fn factory_lock_server(_: *mut c_void, _: BOOL) -> HRESULT {
    S_OK
}

/// Returns the class factory of the thumbnail provider or the preview handler.
///
/// # Safety
/// COM supplies valid class and interface IDs and a writable out pointer.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const c_void,
    iid: *const c_void,
    out: *mut *mut c_void,
) -> HRESULT {
    if clsid.is_null() || out.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM supplies a valid class ID.
    let factory = match unsafe { *clsid.cast::<GUID>() } {
        CLSID_THUMBNAIL_PROVIDER => &THUMBNAIL_FACTORY,
        CLSID_PREVIEW_HANDLER => &PREVIEW_FACTORY,
        _ => return CLASS_E_CLASSNOTAVAILABLE,
    };
    // SAFETY: The factory is a static that lives for the whole process.
    unsafe {
        factory_query_interface(
            std::ptr::from_ref(factory).cast_mut().cast(),
            iid.cast(),
            out,
        )
    }
}

/// The DLL stays loaded, its factories are statics.
#[unsafe(no_mangle)]
pub const extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}
