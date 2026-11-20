/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Read-only image and photo metadata for Explorer's details pane and Properties dialog.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::Mutex;

use imageview_core::decode_media;

use self::values::{Value, image_properties};

use crate::headers::*;
use crate::{ComObject, Interfaces, Stream, guard, read_stream, unknown};

mod values;

#[cfg(test)]
#[path = "../../../../lib/graphics/image/tests/support/metadata.rs"]
mod fixtures;

const STG_E_ACCESSDENIED: HRESULT = 0x8003_0005_u32 as i32;
const IID_IPROPERTY_STORE: GUID = GUID::new(
    0x886d_8eeb,
    0x8cf2,
    0x4446,
    [0x8d, 0x02, 0xcd, 0xba, 0x1d, 0xbd, 0xcf, 0x99],
);
const IID_IPROPERTY_STORE_CAPABILITIES: GUID = GUID::new(
    0xc8e2_d566,
    0x186e,
    0x4d49,
    [0xbf, 0x41, 0x69, 0x09, 0xea, 0xd5, 0x6a, 0xcc],
);

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct PropertyKey {
    format: GUID,
    id: u32,
}

const fn image_key(id: u32) -> PropertyKey {
    PropertyKey {
        format: GUID::new(
            0x6444_048f,
            0x4c8b,
            0x11d1,
            [0x8b, 0x70, 0x08, 0x00, 0x36, 0xb1, 0x1a, 0x03],
        ),
        id,
    }
}

// PKEY_Image_Dimensions (VT_LPWSTR), HorizontalSize and VerticalSize (VT_UI4).
const KEYS: [PropertyKey; 3] = [image_key(13), image_key(3), image_key(4)];

// The counted-pointer arm gives PROPVARIANT its native size on both 32 and 64 bit Windows.
#[repr(C)]
#[derive(Clone, Copy)]
struct CountedPointer {
    count: u32,
    pointer: *mut c_void,
}

#[repr(C)]
union PropertyValue {
    unsigned: u32,
    short: u16,
    signed_short: i16,
    double: f64,
    ticks: u64,
    wide: *mut u16,
    counted: CountedPointer,
    alignment: u64,
}

#[repr(C)]
struct PropVariant {
    kind: u16,
    reserved: [u16; 3],
    value: PropertyValue,
}

impl PropVariant {
    const fn empty() -> Self {
        Self {
            kind: 0,
            reserved: [0; 3],
            value: PropertyValue {
                counted: CountedPointer {
                    count: 0,
                    pointer: std::ptr::null_mut(),
                },
            },
        }
    }
}

#[link(name = "shlwapi")]
unsafe extern "system" {
    fn SHStrDupW(value: *const u16, copy: *mut *mut u16) -> HRESULT;
}

#[link(name = "propsys")]
unsafe extern "system" {
    fn InitPropVariantFromStringVector(
        values: *const *const u16,
        count: u32,
        out: *mut PropVariant,
    ) -> HRESULT;
}

#[repr(C)]
struct InitializeVtbl {
    base: IUnknownVtbl,
    Initialize: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> HRESULT,
}

#[repr(C)]
struct PropertyStoreVtbl {
    base: IUnknownVtbl,
    GetCount: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    GetAt: unsafe extern "system" fn(*mut c_void, u32, *mut PropertyKey) -> HRESULT,
    GetValue:
        unsafe extern "system" fn(*mut c_void, *const PropertyKey, *mut PropVariant) -> HRESULT,
    SetValue:
        unsafe extern "system" fn(*mut c_void, *const PropertyKey, *const PropVariant) -> HRESULT,
    Commit: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

#[repr(C)]
struct CapabilitiesVtbl {
    base: IUnknownVtbl,
    IsPropertyWritable: unsafe extern "system" fn(*mut c_void, *const PropertyKey) -> HRESULT,
}

struct Properties {
    values: Mutex<Option<PropertyData>>,
}

struct PropertyData {
    _stream: Stream,
    values: Vec<(PropertyKey, Value)>,
}

impl Interfaces<3> for Properties {
    const IIDS: [GUID; 3] = [
        IID_IINITIALIZE_WITH_STREAM,
        IID_IPROPERTY_STORE,
        IID_IPROPERTY_STORE_CAPABILITIES,
    ];
}

type Object = ComObject<Properties, 3>;

static INITIALIZE_VTABLE: InitializeVtbl = InitializeVtbl {
    base: unknown::<Properties, 3, 0>(),
    Initialize: initialize,
};
static STORE_VTABLE: PropertyStoreVtbl = PropertyStoreVtbl {
    base: unknown::<Properties, 3, 1>(),
    GetCount: get_count,
    GetAt: get_at,
    GetValue: get_value,
    SetValue: set_value,
    Commit: commit,
};
static CAPABILITIES_VTABLE: CapabilitiesVtbl = CapabilitiesVtbl {
    base: unknown::<Properties, 3, 2>(),
    IsPropertyWritable: is_property_writable,
};

/// Creates a read-only property handler with one reference.
pub(crate) fn create() -> *mut c_void {
    Object::create(
        [
            (&raw const INITIALIZE_VTABLE).cast(),
            (&raw const STORE_VTABLE).cast(),
            (&raw const CAPABILITIES_VTABLE).cast(),
        ],
        Properties {
            values: Mutex::new(None),
        },
    )
}

unsafe extern "system" fn initialize(this: *mut c_void, stream: *mut c_void, mode: u32) -> HRESULT {
    if stream.is_null() {
        return E_POINTER;
    }
    if mode & 3 != 0 {
        return STG_E_ACCESSDENIED;
    }
    guard(|| {
        // SAFETY: COM calls this with the first interface and a live stream.
        let object = unsafe { Object::from_interface(this, 0) };
        let mut values = object
            .value
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if values.is_some() {
            return ERROR_ALREADY_INITIALIZED;
        }
        // SAFETY: The caller supplies a live IStream.
        let Some(bytes) = (unsafe { read_stream(stream) }) else {
            return E_FAIL;
        };
        let info = match image::inspect(&bytes) {
            Ok(info) => info,
            // Vectors and formats without a signature (like TGA) use the media decoder.
            Err(image::DecodeError::InvalidMagic) => {
                let Ok(media) = decode_media(&bytes) else {
                    return E_FAIL;
                };
                let (width, height) = media.size();
                image::ImageInfo {
                    width: width.round() as u32,
                    height: height.round() as u32,
                    bit_depth: None,
                    metadata: Vec::new(),
                }
            }
            Err(_) => return E_FAIL,
        };
        // SAFETY: The stream stays open until this property store is released.
        *values = Some(PropertyData {
            // SAFETY: The caller's live stream remains open until this store is released.
            _stream: unsafe { Stream::retain(stream) },
            values: image_properties(info),
        });
        S_OK
    })
}

unsafe extern "system" fn get_count(this: *mut c_void, count: *mut u32) -> HRESULT {
    if count.is_null() {
        return E_POINTER;
    }
    guard(|| {
        // SAFETY: COM supplies the second interface and a writable out pointer.
        let object = unsafe { Object::from_interface(this, 1) };
        let values = object
            .value
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // SAFETY: The out pointer was checked above.
        unsafe { *count = values.as_ref().map_or(0, |data| data.values.len() as u32) };
        S_OK
    })
}

unsafe extern "system" fn get_at(this: *mut c_void, index: u32, key: *mut PropertyKey) -> HRESULT {
    if key.is_null() {
        return E_POINTER;
    }
    guard(|| {
        // SAFETY: COM supplies the second interface.
        let object = unsafe { Object::from_interface(this, 1) };
        let values = object
            .value
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some((value, _)) = values
            .as_ref()
            .and_then(|data| data.values.get(index as usize))
        else {
            return E_INVALIDARG;
        };
        // SAFETY: The caller supplies a writable out pointer.
        unsafe { *key = *value };
        S_OK
    })
}

unsafe extern "system" fn get_value(
    this: *mut c_void,
    key: *const PropertyKey,
    out: *mut PropVariant,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: The caller supplies a writable out pointer; unknown properties return VT_EMPTY.
    unsafe { *out = PropVariant::empty() };
    if key.is_null() {
        return E_POINTER;
    }
    guard(|| {
        // SAFETY: COM supplies the second interface and a valid property key.
        let (object, key) = unsafe { (Object::from_interface(this, 1), *key) };
        let values = object
            .value
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some((_, value)) = values
            .as_ref()
            .and_then(|data| data.values.iter().find(|(candidate, _)| *candidate == key))
        else {
            return S_OK;
        };
        if let Value::Text(value) = value {
            let text: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
            // SAFETY: The input is null terminated and out is writable. The shell owns the
            // returned string and releases it with PropVariantClear.
            return unsafe {
                let result = SHStrDupW(text.as_ptr(), &raw mut (*out).value.wide);
                if result >= 0 {
                    (*out).kind = 31;
                }
                result
            };
        }
        if let Value::Texts(values) = value {
            let strings: Vec<Vec<u16>> = values
                .iter()
                .map(|text| text.encode_utf16().chain([0]).collect())
                .collect();
            let pointers: Vec<*const u16> = strings.iter().map(|text| text.as_ptr()).collect();
            // SAFETY: The strings are terminated and remain live until the API copies them.
            return unsafe {
                InitPropVariantFromStringVector(pointers.as_ptr(), pointers.len() as u32, out)
            };
        }
        // SAFETY: The output is writable; each VARTYPE matches the corresponding union arm.
        unsafe {
            match value {
                Value::Unsigned(value) => {
                    (*out).kind = 19;
                    (*out).value.unsigned = *value;
                }
                Value::Short(value) => {
                    (*out).kind = 18;
                    (*out).value.short = *value;
                }
                Value::SignedShort(value) => {
                    (*out).kind = 2;
                    (*out).value.signed_short = *value;
                }
                Value::Double(value) => {
                    (*out).kind = 5;
                    (*out).value.double = *value;
                }
                Value::FileTime(value) => {
                    (*out).kind = 64;
                    (*out).value.ticks = *value;
                }
                Value::Text(_) | Value::Texts(_) => unreachable!(),
            }
        }
        S_OK
    })
}

const unsafe extern "system" fn set_value(
    _: *mut c_void,
    _: *const PropertyKey,
    _: *const PropVariant,
) -> HRESULT {
    STG_E_ACCESSDENIED
}

const unsafe extern "system" fn commit(_: *mut c_void) -> HRESULT {
    STG_E_ACCESSDENIED
}

const unsafe extern "system" fn is_property_writable(
    _: *mut c_void,
    _: *const PropertyKey,
) -> HRESULT {
    S_FALSE
}

#[cfg(test)]
mod tests {
    use std::ptr::{null, null_mut};

    use super::*;

    #[link(name = "shlwapi")]
    unsafe extern "system" {
        fn SHCreateMemStream(bytes: *const u8, size: u32) -> *mut c_void;
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, mode: u32) -> HRESULT;
        fn CoUninitialize();
        fn PropVariantClear(value: *mut PropVariant) -> HRESULT;
    }

    #[link(name = "propsys")]
    unsafe extern "system" {
        fn PropVariantToString(value: *const PropVariant, text: *mut u16, size: u32) -> HRESULT;
        fn PSGetPropertyDescription(
            key: *const PropertyKey,
            iid: *const GUID,
            description: *mut *mut c_void,
        ) -> HRESULT;
    }

    #[repr(C)]
    struct DescriptionVtbl {
        base: IUnknownVtbl,
        GetPropertyKey: unsafe extern "system" fn(*mut c_void, *mut PropertyKey) -> HRESULT,
        GetCanonicalName: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
        GetPropertyType: unsafe extern "system" fn(*mut c_void, *mut u16) -> HRESULT,
    }

    // Ask Windows itself for the expected VARTYPE rather than repeating the schema in a test.
    fn schema_type(key: &PropertyKey) -> u16 {
        let iid = GUID::new(
            0x6f79_d558,
            0x3e96,
            0x4549,
            [0xa1, 0xd1, 0x7d, 0x75, 0xd2, 0x28, 0x88, 0x14],
        );
        let mut description = null_mut();
        // SAFETY: The key and IID are valid; the returned description is released below.
        unsafe {
            assert_eq!(PSGetPropertyDescription(key, &iid, &mut description), S_OK);
            let vtable = &**description.cast::<*const DescriptionVtbl>();
            let mut kind = 0;
            let result = (vtable.GetPropertyType)(description, &mut kind);
            (vtable.base.Release)(description);
            assert_eq!(result, S_OK);
            kind
        }
    }

    #[test]
    fn photo_metadata_matches_the_native_windows_schema() {
        for little in [false, true] {
            let exif = fixtures::photo_tiff(little);
            for bytes in [
                exif.clone(),
                fixtures::photo_jpeg(&exif),
                fixtures::photo_png(&exif),
                fixtures::photo_webp(&exif),
            ] {
                let handler = Handler::new();
                assert_eq!(handler.initialize(&bytes, 0), S_OK);
                // SAFETY: The handler is live, all output buffers are writable, and each
                // returned PROPVARIANT is released after its value has been checked.
                unsafe {
                    let vtable = &**handler.store.cast::<*const PropertyStoreVtbl>();
                    let mut count = 0;
                    assert_eq!((vtable.GetCount)(handler.store, &mut count), S_OK);
                    assert!(count >= 20, "missing metadata: {count}");
                    for index in 0..count {
                        let mut key = image_key(0);
                        assert_eq!((vtable.GetAt)(handler.store, index, &mut key), S_OK);
                        let mut value = PropVariant::empty();
                        assert_eq!((vtable.GetValue)(handler.store, &key, &mut value), S_OK);
                        assert_eq!(value.kind, schema_type(&key));
                        assert_eq!(PropVariantClear(&mut value), S_OK);
                    }
                    for (name, expected) in [
                        ("System.Photo.ExposureTime", 1.0 / 125.0),
                        ("System.Photo.FNumber", 2.8),
                        ("System.Photo.ExposureBias", -1.0 / 3.0),
                        ("System.Photo.FocalLength", 35.1),
                        ("System.Image.HorizontalResolution", 300.0),
                        ("System.Image.VerticalResolution", 300.0),
                    ] {
                        let key = values::named_key(name).expect("property key");
                        let mut value = PropVariant::empty();
                        assert_eq!((vtable.GetValue)(handler.store, &key, &mut value), S_OK);
                        assert_eq!(value.kind, 5, "{name}");
                        assert!((value.value.double - expected).abs() < 1e-12, "{name}");
                        PropVariantClear(&mut value);
                    }
                    for (name, expected, kind) in [
                        ("System.Photo.CameraManufacturer", "Acme", 31),
                        ("System.Photo.CameraModel", "Camera X", 31),
                        ("System.Author", "Jane Doe", 0x101f),
                        ("System.Copyright", "Copyright 2026 Jane Doe", 31),
                        ("System.Title", "A & B 🐈", 31),
                    ] {
                        let key = values::named_key(name).expect("property key");
                        let mut value = PropVariant::empty();
                        assert_eq!((vtable.GetValue)(handler.store, &key, &mut value), S_OK);
                        assert_eq!(value.kind, kind, "{name}");
                        let mut text = [0u16; 128];
                        assert_eq!(
                            PropVariantToString(&value, text.as_mut_ptr(), text.len() as u32),
                            S_OK
                        );
                        let end = text.iter().position(|unit| *unit == 0).expect("terminator");
                        assert_eq!(String::from_utf16(&text[..end]).expect("UTF-16"), expected);
                        PropVariantClear(&mut value);
                    }
                    let key = values::named_key("System.Photo.DateTaken").expect("date key");
                    let mut value = PropVariant::empty();
                    assert_eq!((vtable.GetValue)(handler.store, &key, &mut value), S_OK);
                    assert_eq!(value.kind, 64);
                    assert_eq!(value.value.ticks, 133_536_764_960_000_000);
                    PropVariantClear(&mut value);
                }
            }
        }
    }

    #[test]
    fn png_text_resolution_and_missing_camera_fields() {
        let mut bytes = fixtures::photo_png(&[]);
        bytes.truncate(bytes.len() - 12);
        fixtures::png_chunk(&mut bytes, b"pHYs", &[0, 0, 46, 35, 0, 0, 46, 35, 1]);
        fixtures::png_chunk(&mut bytes, b"tEXt", b"Author\0A PNG artist");
        fixtures::png_chunk(&mut bytes, b"tEXt", b"Copyright\0A PNG copyright");
        fixtures::png_chunk(&mut bytes, b"IEND", &[]);
        let handler = Handler::new();
        assert_eq!(handler.initialize(&bytes, 0), S_OK);
        // SAFETY: The handler and every output buffer are live. Each allocated value is cleared.
        unsafe {
            let vtable = &**handler.store.cast::<*const PropertyStoreVtbl>();
            for name in [
                "System.Image.HorizontalResolution",
                "System.Image.VerticalResolution",
            ] {
                let key = values::named_key(name).expect("resolution key");
                let mut value = PropVariant::empty();
                assert_eq!((vtable.GetValue)(handler.store, &key, &mut value), S_OK);
                assert_eq!(value.kind, 5);
                assert!((value.value.double - 299.9994).abs() < 1e-9);
                PropVariantClear(&mut value);
            }
            for (name, kind) in [
                ("System.Author", 0x101f),
                ("System.Copyright", 31),
                ("System.Photo.CameraModel", 0),
                ("System.Photo.DateTaken", 0),
            ] {
                let key = values::named_key(name).expect("key");
                let mut value = PropVariant::empty();
                assert_eq!((vtable.GetValue)(handler.store, &key, &mut value), S_OK);
                assert_eq!(value.kind, kind, "{name}");
                PropVariantClear(&mut value);
            }
        }
    }

    #[test]
    fn property_store_keeps_the_source_stream_open() {
        use crate::test_support::{Apartment, Interface, MemoryStream};
        let _apartment = Apartment::new();
        let stream = MemoryStream::new(include_bytes!("../../imageview/tests/fixtures/rgb.qoi"));
        let handler =
            Interface::create(&crate::CLSID_PROPERTY_HANDLER, &IID_IINITIALIZE_WITH_STREAM);
        // SAFETY: Both interfaces are live and Initialize receives a valid IStream.
        assert_eq!(unsafe { initialize(handler.0, stream.0.0, 0) }, S_OK);
        assert_eq!(stream.references(), 2);
        drop(handler);
        assert_eq!(stream.references(), 1);
    }

    #[test]
    fn xmp_capture_date_preserves_fractional_seconds_and_offset() {
        let mut bytes = fixtures::photo_png(&[]);
        bytes.truncate(bytes.len() - 12);
        fixtures::png_chunk(&mut bytes, b"tEXt", b"XML:com.adobe.xmp\0<rdf:Description xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#' xmlns:xmp='http://ns.adobe.com/xap/1.0/' xmp:CreateDate='2024-02-29T12:34:56.1234567+02:00'/>");
        fixtures::png_chunk(&mut bytes, b"IEND", &[]);
        let handler = Handler::new();
        assert_eq!(handler.initialize(&bytes, 0), S_OK);
        let key = values::named_key("System.Photo.DateTaken").expect("date key");
        let mut value = PropVariant::empty();
        // SAFETY: The store, key and output buffer are valid. The value is cleared afterward.
        unsafe {
            assert_eq!(get_value(handler.store, &key, &mut value), S_OK);
            assert_eq!(value.kind, 64);
            assert_eq!(value.value.ticks, 133_536_764_961_234_567);
            PropVariantClear(&mut value);
        }
    }

    struct Handler {
        initialize: *mut c_void,
        store: *mut c_void,
        capabilities: *mut c_void,
    }

    impl Handler {
        fn new() -> Self {
            // SAFETY: Each test initializes COM on its own thread and drops the handler before
            // uninitializing it. The factory returns owned interface references.
            unsafe {
                assert!(CoInitializeEx(null_mut(), 2) >= 0);
                let mut factory = null_mut();
                assert_eq!(
                    crate::DllGetClassObject(
                        std::ptr::from_ref(&crate::CLSID_PROPERTY_HANDLER).cast(),
                        std::ptr::from_ref(&IID_ICLASS_FACTORY).cast(),
                        &mut factory,
                    ),
                    S_OK
                );
                let vtable = &**factory.cast::<*const crate::IClassFactoryVtbl>();
                let mut initialize = null_mut();
                assert_eq!(
                    (vtable.CreateInstance)(
                        factory,
                        null_mut(),
                        &IID_IINITIALIZE_WITH_STREAM,
                        &mut initialize
                    ),
                    S_OK
                );
                (vtable.base.Release)(factory);
                let unknown = &**initialize.cast::<*const IUnknownVtbl>();
                let mut store = null_mut();
                assert_eq!(
                    (unknown.QueryInterface)(initialize, &IID_IPROPERTY_STORE, &mut store),
                    S_OK
                );
                let mut capabilities = null_mut();
                assert_eq!(
                    (unknown.QueryInterface)(
                        initialize,
                        &IID_IPROPERTY_STORE_CAPABILITIES,
                        &mut capabilities
                    ),
                    S_OK
                );
                Self {
                    initialize,
                    store,
                    capabilities,
                }
            }
        }

        fn initialize(&self, bytes: &[u8], mode: u32) -> HRESULT {
            // SAFETY: SHCreateMemStream copies the valid input; both interfaces remain live.
            unsafe {
                let stream = SHCreateMemStream(bytes.as_ptr(), bytes.len() as u32);
                assert!(!stream.is_null());
                let vtable = &**self.initialize.cast::<*const InitializeVtbl>();
                let result = (vtable.Initialize)(self.initialize, stream, mode);
                let stream_vtable = &**stream.cast::<*const IUnknownVtbl>();
                (stream_vtable.Release)(stream);
                result
            }
        }

        fn dimensions(&self, width: u32, height: u32) {
            // SAFETY: The handler is live and every out parameter points to initialized storage.
            unsafe {
                let vtable = &**self.store.cast::<*const PropertyStoreVtbl>();
                let mut count = 0;
                assert_eq!((vtable.GetCount)(self.store, &mut count), S_OK);
                assert_eq!(count, 3);
                for (index, expected) in [
                    format!("{width} x {height}"),
                    width.to_string(),
                    height.to_string(),
                ]
                .iter()
                .enumerate()
                {
                    let mut key = image_key(0);
                    assert_eq!((vtable.GetAt)(self.store, index as u32, &mut key), S_OK);
                    assert!(key == KEYS[index]);
                    let mut value = PropVariant::empty();
                    assert_eq!((vtable.GetValue)(self.store, &key, &mut value), S_OK);
                    assert_eq!(value.kind, if index == 0 { 31 } else { 19 });
                    let mut text = [0u16; 128];
                    assert_eq!(
                        PropVariantToString(&value, text.as_mut_ptr(), text.len() as u32),
                        S_OK
                    );
                    let end = text.iter().position(|unit| *unit == 0).expect("Terminator");
                    assert_eq!(String::from_utf16(&text[..end]).expect("UTF-16"), *expected);
                    assert_eq!(PropVariantClear(&mut value), S_OK);
                }
                let mut key = image_key(0);
                assert_eq!((vtable.GetAt)(self.store, 3, &mut key), E_INVALIDARG);
                let mut value = PropVariant::empty();
                assert_eq!(
                    (vtable.GetValue)(self.store, &image_key(999), &mut value),
                    S_OK
                );
                assert_eq!(value.kind, 0);
                assert_eq!(
                    (vtable.SetValue)(self.store, &KEYS[1], &value),
                    STG_E_ACCESSDENIED
                );
                assert_eq!((vtable.Commit)(self.store), STG_E_ACCESSDENIED);
                let capabilities = &**self.capabilities.cast::<*const CapabilitiesVtbl>();
                assert_eq!(
                    (capabilities.IsPropertyWritable)(self.capabilities, &KEYS[1]),
                    S_FALSE
                );
            }
        }
    }

    impl Drop for Handler {
        fn drop(&mut self) {
            // SAFETY: The test owns one reference to each interface and initialized COM once.
            unsafe {
                for interface in [self.initialize, self.store, self.capabilities] {
                    let vtable = &**interface.cast::<*const IUnknownVtbl>();
                    (vtable.Release)(interface);
                }
                CoUninitialize();
            }
        }
    }

    #[test]
    fn property_handler_exposes_typed_read_only_dimensions() {
        assert_eq!(size_of::<PropVariant>(), 24);
        let handler = Handler::new();
        let bytes = include_bytes!("../../imageview/tests/fixtures/rgb.qoi");
        assert_eq!(handler.initialize(bytes, 0), S_OK);
        handler.dimensions(19, 13);
        assert_eq!(handler.initialize(bytes, 0), ERROR_ALREADY_INITIALIZED);
        handler.dimensions(19, 13);
    }

    #[test]
    fn property_handler_rejects_invalid_input_and_write_access() {
        let handler = Handler::new();
        let bytes = include_bytes!("../../imageview/tests/fixtures/rgb.qoi");
        assert_eq!(handler.initialize(bytes, 2), STG_E_ACCESSDENIED);
        assert_eq!(handler.initialize(b"not an image", 0), E_FAIL);
        // SAFETY: The test supplies live interfaces and intentionally null optional outputs.
        unsafe {
            assert_eq!(initialize(handler.initialize, null_mut(), 0), E_POINTER);
            assert_eq!(get_count(handler.store, null_mut()), E_POINTER);
            assert_eq!(get_at(handler.store, 0, null_mut()), E_POINTER);
            assert_eq!(get_value(handler.store, null(), null_mut()), E_POINTER);
            let mut count = 99;
            assert_eq!(get_count(handler.store, &mut count), S_OK);
            assert_eq!(count, 0);
        }
        assert_eq!(handler.initialize(bytes, 0), S_OK);
        handler.dimensions(19, 13);
    }

    #[test]
    fn property_handler_reads_vector_dimensions() {
        let handler = Handler::new();
        assert_eq!(
            handler.initialize(
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="37" height="23"/>"#,
                0
            ),
            S_OK
        );
        handler.dimensions(37, 23);
    }

    #[test]
    fn property_handler_uses_largest_icon_size() {
        let bytes = include_bytes!("../../imageview/meta/windows/file-types/qoi.ico");
        let handler = Handler::new();
        assert_eq!(handler.initialize(bytes, 0), S_OK);
        handler.dimensions(256, 256);
    }
}
