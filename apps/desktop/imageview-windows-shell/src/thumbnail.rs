/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The thumbnail provider, which renders the media into a bitmap with alpha.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Mutex;

use imageview_core::{decode_media, render_thumbnail};

use crate::headers::*;
use crate::{ComObject, Interfaces, Stream, guard, read_stream, unknown};

struct Thumbnail {
    bytes: Mutex<Option<(Stream, Vec<u8>)>>,
}

impl Interfaces<2> for Thumbnail {
    const IIDS: [GUID; 2] = [IID_IINITIALIZE_WITH_STREAM, IID_ITHUMBNAIL_PROVIDER];
}

type Object = ComObject<Thumbnail, 2>;

#[repr(C)]
struct IInitializeWithStreamVtbl {
    base: IUnknownVtbl,
    Initialize: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> HRESULT,
}

#[repr(C)]
struct IThumbnailProviderVtbl {
    base: IUnknownVtbl,
    GetThumbnail:
        unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void, *mut u32) -> HRESULT,
}

static INITIALIZE_VTABLE: IInitializeWithStreamVtbl = IInitializeWithStreamVtbl {
    base: unknown::<Thumbnail, 2, 0>(),
    Initialize: initialize,
};

static THUMBNAIL_VTABLE: IThumbnailProviderVtbl = IThumbnailProviderVtbl {
    base: unknown::<Thumbnail, 2, 1>(),
    GetThumbnail: get_thumbnail,
};

/// Creates a thumbnail provider with one reference.
pub(crate) fn create() -> *mut c_void {
    Object::create(
        [
            (&raw const INITIALIZE_VTABLE).cast(),
            (&raw const THUMBNAIL_VTABLE).cast(),
        ],
        Thumbnail {
            bytes: Mutex::new(None),
        },
    )
}

unsafe extern "system" fn initialize(this: *mut c_void, stream: *mut c_void, _: u32) -> HRESULT {
    if stream.is_null() {
        return E_POINTER;
    }
    guard(|| {
        // SAFETY: COM calls this with the first interface and a live stream.
        let object = unsafe { Object::from_interface(this, 0) };
        let mut stored = object
            .value
            .bytes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if stored.is_some() {
            return ERROR_ALREADY_INITIALIZED;
        }
        // SAFETY: The caller supplies a live IStream.
        let bytes = unsafe { read_stream(stream) };
        let Some(bytes) = bytes else {
            return E_FAIL;
        };
        // SAFETY: The source remains open for the lifetime of this initialized handler.
        *stored = Some((unsafe { Stream::retain(stream) }, bytes));
        S_OK
    })
}

unsafe extern "system" fn get_thumbnail(
    this: *mut c_void,
    size: u32,
    bitmap: *mut *mut c_void,
    alpha: *mut u32,
) -> HRESULT {
    // SAFETY: Initialize each supplied output before any operation that can fail.
    unsafe {
        if !bitmap.is_null() {
            *bitmap = null_mut();
        }
        if !alpha.is_null() {
            *alpha = 0;
        }
    }
    if bitmap.is_null() || alpha.is_null() {
        return E_POINTER;
    }
    guard(|| {
        // SAFETY: COM calls this with the second interface.
        let object = unsafe { Object::from_interface(this, 1) };
        let bytes = object
            .value
            .bytes
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(thumbnail) = bytes
            .as_ref()
            .map(|(_, bytes)| bytes.as_slice())
            .and_then(|bytes| decode_media(bytes).ok())
            .and_then(|media| render_thumbnail(media, size))
        else {
            return E_FAIL;
        };
        let Some(native) = create_bitmap(&thumbnail) else {
            return E_FAIL;
        };
        // SAFETY: The out pointers were checked, the caller owns the bitmap.
        unsafe {
            *bitmap = native;
            *alpha = WTSAT_ARGB;
        }
        S_OK
    })
}

// Creates a top-down 32-bit bitmap with premultiplied alpha.
fn create_bitmap(thumbnail: &image::Bitmap) -> Option<*mut c_void> {
    let (width, height) = (thumbnail.width(), thumbnail.height());
    let info = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: -(height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let mut bits = null_mut();
    // SAFETY: The info describes a 32-bit top-down DIB, the system allocates its bits.
    let bitmap = unsafe { CreateDIBSection(null_mut(), &info, 0, &mut bits, null_mut(), 0) };
    if bitmap.is_null() || bits.is_null() {
        return None;
    }
    let length = width as usize * height as usize * 4;
    // SAFETY: The DIB has room for every pixel of the thumbnail.
    let output = unsafe { std::slice::from_raw_parts_mut(bits.cast::<u8>(), length) };
    for (pixel, output) in thumbnail
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(output.as_chunks_mut::<4>().0)
    {
        let alpha = u16::from(pixel[3]);
        let premultiply = |value: u8| ((u16::from(value) * alpha + 127) / 255) as u8;
        *output = [
            premultiply(pixel[2]),
            premultiply(pixel[1]),
            premultiply(pixel[0]),
            pixel[3],
        ];
    }
    Some(bitmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Apartment, Interface, MemoryStream};

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn DeleteObject(object: *mut c_void) -> BOOL;
    }

    #[test]
    fn thumbnail_initialization_outputs_and_stream_lifetime() {
        let _apartment = Apartment::new();
        let initialize_interface = Interface::create(
            &crate::CLSID_THUMBNAIL_PROVIDER,
            &IID_IINITIALIZE_WITH_STREAM,
        );
        let thumbnail = initialize_interface.query(&IID_ITHUMBNAIL_PROVIDER);
        let stream = MemoryStream::new(include_bytes!("../../imageview/tests/fixtures/rgb.qoi"));
        // SAFETY: All interfaces are live and all output pointers are writable.
        unsafe {
            let mut bitmap = std::ptr::dangling_mut();
            let mut alpha = 99;
            assert_eq!(
                get_thumbnail(thumbnail.0, 64, &mut bitmap, &mut alpha),
                E_FAIL
            );
            assert!(bitmap.is_null());
            assert_eq!(alpha, 0);
            assert_eq!(
                get_thumbnail(thumbnail.0, 64, null_mut(), &mut alpha),
                E_POINTER
            );
            bitmap = std::ptr::dangling_mut();
            assert_eq!(
                get_thumbnail(thumbnail.0, 64, &mut bitmap, null_mut()),
                E_POINTER
            );
            assert!(bitmap.is_null());
            assert_eq!(initialize(initialize_interface.0, null_mut(), 0), E_POINTER);
            assert_eq!(stream.references(), 1);
            assert_eq!(initialize(initialize_interface.0, stream.0.0, 0), S_OK);
            assert_eq!(stream.references(), 2);
            assert_eq!(
                initialize(initialize_interface.0, stream.0.0, 0),
                ERROR_ALREADY_INITIALIZED
            );
            assert_eq!(
                get_thumbnail(thumbnail.0, 64, &mut bitmap, &mut alpha),
                S_OK
            );
            assert!(!bitmap.is_null());
            assert_eq!(alpha, WTSAT_ARGB);
            assert_ne!(DeleteObject(bitmap), 0);
        }
        drop(thumbnail);
        drop(initialize_interface);
        assert_eq!(stream.references(), 1);
    }
}
