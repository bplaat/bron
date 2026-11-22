/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Handwritten WinRT bindings for the Windows App SDK: reference counted interface pointers,
//! strings, activation and the COM objects ImageView implements itself. Partial vtables
//! preserve the interface layouts defined by the Windows App SDK metadata.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::mem::{forget, offset_of};
use std::ptr::{NonNull, from_ref, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};

use super::headers::{w, wide};

pub(crate) type HRESULT = i32;
pub(crate) type HSTRING = *mut c_void;
pub(crate) type Result<T> = std::result::Result<T, HRESULT>;

const S_OK: HRESULT = 0;
const E_NOINTERFACE: HRESULT = 0x8000_4002_u32 as i32;
pub(crate) const E_POINTER: HRESULT = 0x8000_4003_u32 as i32;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct GUID {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

impl GUID {
    pub(crate) const fn from_u128(value: u128) -> Self {
        Self {
            data1: (value >> 96) as u32,
            data2: (value >> 80) as u16,
            data3: (value >> 64) as u16,
            data4: (value as u64).to_be_bytes(),
        }
    }
}

pub(crate) const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
pub(crate) const IID_IINSPECTABLE: GUID = GUID::from_u128(0xaf86e2e0_b12d_4c6a_9c5a_d7aa65101e90);
pub(crate) const IID_IAGILE_OBJECT: GUID = GUID::from_u128(0x94ea2b94_e9cc_49e0_c0ff_ee64ca8f5b90);
pub(crate) const IID_ICLOSABLE: GUID = GUID::from_u128(0x30d5a829_7fa4_4026_83bb_d75bae4ea99e);
pub(crate) const IID_IAPPLICATION_FACTORY: GUID =
    GUID::from_u128(0x9fd96657_5294_5a65_a1db_4fea143597da);
pub(crate) const IID_IAPPLICATION_OVERRIDES: GUID =
    GUID::from_u128(0xa33e81ef_c665_503b_8827_d27ef1720a06);
pub(crate) const IID_IXAML_METADATA_PROVIDER: GUID =
    GUID::from_u128(0xa96251f0_2214_5d53_8746_ce99a2593cd7);
pub(crate) const IID_IDISPATCHER_QUEUE_CONTROLLER_STATICS: GUID =
    GUID::from_u128(0xf18d6145_722b_593d_bcf2_a61e713f0037);
pub(crate) const IID_IWINDOWS_XAML_MANAGER_STATICS: GUID =
    GUID::from_u128(0x56cb591d_de97_539f_881d_8ccdc44fa6c4);
pub(crate) const IID_IDESKTOP_WINDOW_XAML_SOURCE_FACTORY: GUID =
    GUID::from_u128(0x7d2db617_14e7_5d49_aeec_ae10887e595d);
pub(crate) const IID_IDESKTOP_SITE_BRIDGE: GUID =
    GUID::from_u128(0xf0ae8750_905c_50a2_8a12_4545c6245bb4);
pub(crate) const IID_IXAML_READER_STATICS: GUID =
    GUID::from_u128(0x82a4cd9e_435e_5aeb_8c4f_300cece45cae);
pub(crate) const IID_IUI_ELEMENT: GUID = GUID::from_u128(0xc3c01020_320c_5cf6_9d24_d396bbfa4d8b);
pub(crate) const IID_IFRAMEWORK_ELEMENT: GUID =
    GUID::from_u128(0xfe08f13d_dc6a_5495_ad44_c2d8d21863b0);
pub(crate) const IID_IBUTTON_BASE: GUID = GUID::from_u128(0x65714269_2473_5327_a652_0ea6bce7f403);
pub(crate) const IID_ITEXT_BLOCK: GUID = GUID::from_u128(0x1ac8d84f_392c_5c7e_83f5_a53e3bf0abb0);
pub(crate) const IID_IRESOURCE_DICTIONARY: GUID =
    GUID::from_u128(0x1b690975_a710_5783_a6e1_15836f6186c2);
pub(crate) const IID_ISYSTEM_BACKDROP: GUID =
    GUID::from_u128(0x5aeed5c4_37ac_5852_b73f_1b76ebc3205f);
pub(crate) const IID_IMICA_BACKDROP_FACTORY: GUID =
    GUID::from_u128(0x774379ce_74bd_59d4_849d_d99c4184d838);
pub(crate) const IID_IAPP_WINDOW_STATICS: GUID =
    GUID::from_u128(0x3c315c24_d540_5d72_b518_b226b83627cb);
pub(crate) const IID_IAPP_WINDOW_TITLE_BAR2: GUID =
    GUID::from_u128(0x86faed38_748a_5b4b_9ccf_3ba0496c9041);
pub(crate) const IID_IINPUT_NON_CLIENT_POINTER_SOURCE_STATICS: GUID =
    GUID::from_u128(0x7d0b775c_1903_5dc7_bd2f_7a4b31f0cff2);
pub(crate) const IID_IWRITEABLE_BITMAP_FACTORY: GUID =
    GUID::from_u128(0x26e861d9_b080_512b_96c4_80050e7e08d1);
pub(crate) const IID_IWRITEABLE_BITMAP: GUID =
    GUID::from_u128(0x78c824a9_0e43_5f1e_93bc_d046cca82b7e);
pub(crate) const IID_IIMAGE_SOURCE: GUID = GUID::from_u128(0x6c2038f6_d6d5_55e9_9b9e_082f12dbff60);
pub(crate) const IID_IIMAGE: GUID = GUID::from_u128(0x220d3d8d_66de_53a1_a215_ba9c165565ab);
pub(crate) const IID_IBUFFER_BYTE_ACCESS: GUID =
    GUID::from_u128(0x905a0fef_bc53_11df_8c49_001e4fc686da);
pub(crate) const IID_ISCROLL_VIEWER: GUID = GUID::from_u128(0x1dc28c2e_996c_5394_89c3_4dc656b4ad46);
pub(crate) const IID_ISCROLL_VIEWER_VIEW_CHANGED_EVENT_ARGS: GUID =
    GUID::from_u128(0xbf7bb85b_1d46_5004_a370_ecb626630588);
// EventHandler<ScrollViewerViewChangedEventArgs>, derived from its parameterized type signature.
pub(crate) const IID_VIEW_CHANGED_EVENT_HANDLER: GUID =
    GUID::from_u128(0xd792e426_9f8e_544a_9a21_9836aabe896b);
pub(crate) const IID_POINTER_EVENT_HANDLER: GUID =
    GUID::from_u128(0xa48a71e1_8bb4_5597_9e31_903a3f6a04fb);
pub(crate) const IID_IPOINTER_ROUTED_EVENT_ARGS: GUID =
    GUID::from_u128(0x66e78a9a_1bec_5f92_b1a1_ea6334ee511c);
pub(crate) const IID_IPROPERTY_VALUE_STATICS: GUID =
    GUID::from_u128(0x629bdbc8_d932_4ff4_96b9_8d96c5c1e858);
// IReference<Double> and IReference<Single>, derived from their parameterized type signatures.
pub(crate) const IID_IREFERENCE_DOUBLE: GUID =
    GUID::from_u128(0x2f2d6c29_5473_5f3e_92e7_96572bb990e2);
pub(crate) const IID_IREFERENCE_SINGLE: GUID =
    GUID::from_u128(0x719cc2ba_3e76_5def_9f1a_38d85a145ea8);
pub(crate) const IID_ROUTED_EVENT_HANDLER: GUID =
    GUID::from_u128(0xdae23d85_69ca_5bdf_805b_6161a3a215cc);
pub(crate) const IID_SIZE_CHANGED_EVENT_HANDLER: GUID =
    GUID::from_u128(0x8d7b1a58_14c6_51c9_892c_9fcce368e77d);
pub(crate) const IID_ICOMPOSITION_TARGET_STATICS: GUID =
    GUID::from_u128(0x12a4be6f_6db1_5165_b622_d57ab782745b);
// EventHandler<Microsoft.UI.Xaml.Media.RenderedEventArgs>, derived from its parameterized type
// signature.
pub(crate) const IID_RENDERED_EVENT_HANDLER: GUID =
    GUID::from_u128(0x19a913f7_9793_5d2e_b989_7ee26c263a2e);
#[cfg(feature = "edit")]
pub(crate) const IID_KEY_EVENT_HANDLER: GUID =
    GUID::from_u128(0xdb68e7cc_9a2b_527d_9989_25284daccc03);
#[cfg(feature = "edit")]
pub(crate) const IID_IKEY_ROUTED_EVENT_ARGS: GUID =
    GUID::from_u128(0xee357007_a2d6_5c75_9431_05fd66ec7915);
#[cfg(feature = "edit")]
pub(crate) const IID_ICONTROL: GUID = GUID::from_u128(0x857d6e8a_d45a_5c69_a99c_bf6a5c54fb38);
pub(crate) const IID_ITOGGLE_BUTTON: GUID = GUID::from_u128(0x686fbaa4_c866_568b_8f75_481d8d545291);
#[cfg(feature = "edit")]
pub(crate) const IID_IRANGE_BASE: GUID = GUID::from_u128(0x540d6d61_8fac_5d5c_b5b0_e172a7dde103);
#[cfg(feature = "edit")]
pub(crate) const IID_ISELECTOR: GUID = GUID::from_u128(0x8f7e2159_e61d_576f_8476_f83fde3d689e);
#[cfg(feature = "edit")]
pub(crate) const IID_INUMBER_BOX: GUID = GUID::from_u128(0xc18eb0e9_29fb_525d_abbc_d6b2110f542e);
pub(crate) const IID_IBORDER: GUID = GUID::from_u128(0x1ca13b47_ff5c_5abc_a411_a177df9482a9);
pub(crate) const IID_IMENU_FLYOUT_ITEM: GUID =
    GUID::from_u128(0x4252df5a_44f9_5ee8_b1cc_53de9aaa4095);
#[cfg(feature = "edit")]
pub(crate) const IID_RANGE_BASE_VALUE_CHANGED_EVENT_HANDLER: GUID =
    GUID::from_u128(0x23f0e209_9455_54cb_b8bc_0b49553c7dcc);
#[cfg(feature = "edit")]
pub(crate) const IID_SELECTION_CHANGED_EVENT_HANDLER: GUID =
    GUID::from_u128(0xa232390d_0e34_595e_8931_fa928a9909f4);
// TypedEventHandler<NumberBox, NumberBoxValueChangedEventArgs>, derived from its parameterized type
// signature.
#[cfg(feature = "edit")]
pub(crate) const IID_NUMBER_BOX_VALUE_CHANGED_EVENT_HANDLER: GUID =
    GUID::from_u128(0x3afd17ac_1ac8_5e8c_9d28_35576eac9a94);
// IReference<Boolean>, derived from its parameterized type signature.
pub(crate) const IID_IREFERENCE_BOOLEAN: GUID =
    GUID::from_u128(0x3c00fd60_2950_5939_a21a_2d12c5a01b8a);
// IReference<Windows.UI.Color>, derived from its parameterized type signature.
const IID_IREFERENCE_COLOR: GUID = GUID::from_u128(0xab8e5d11_b0c1_5a21_95ae_f16bf3a37624);

/// `Microsoft.UI.WindowId`
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct WindowId {
    pub(crate) value: u64,
}

/// `Windows.Graphics.RectInt32`
#[repr(C)]
pub(crate) struct RectInt32 {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

/// `Microsoft.UI.Xaml.Thickness`
#[repr(C)]
pub(crate) struct Thickness {
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) right: f64,
    pub(crate) bottom: f64,
}

/// `Windows.Foundation.Point`
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct Point {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

/// `Windows.UI.Color`
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Color {
    pub(crate) a: u8,
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
}

/// `Windows.UI.Xaml.Interop.TypeName`
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct TypeName {
    name: HSTRING,
    kind: i32,
}

#[link(name = "runtimeobject")]
unsafe extern "system" {
    fn WindowsCreateString(text: *const u16, length: u32, string: *mut HSTRING) -> HRESULT;
    fn WindowsDeleteString(string: HSTRING) -> HRESULT;
    fn WindowsGetStringRawBuffer(string: HSTRING, length: *mut u32) -> *const u16;
    fn RoGetActivationFactory(
        class: HSTRING,
        iid: *const GUID,
        factory: *mut *mut c_void,
    ) -> HRESULT;
    fn RoActivateInstance(class: HSTRING, instance: *mut *mut c_void) -> HRESULT;
}

/// Turns a failed `HRESULT` into an error.
pub(crate) const fn check(result: HRESULT) -> Result<()> {
    if result < 0 { Err(result) } else { Ok(()) }
}

/// Declares a partial COM interface, with its inherited vtable first.
macro_rules! interface {
    ($(#[$attr:meta])* $interface:ident, $vtable:ident : $base:ty { $($name:ident: $ty:ty,)* }) => {
        $(#[$attr])*
        #[repr(C)]
        pub(super) struct $interface {
            pub(super) vtbl: *const $vtable,
        }

        $(#[$attr])*
        #[allow(non_snake_case)]
        #[repr(C)]
        pub(super) struct $vtable {
            base: $base,
            $(pub(super) $name: $ty,)*
        }
    };
}
pub(super) use interface;

/// Calls a named entry on the specified COM interface.
/// The caller must hold that interface on the current apartment and satisfy the method contract.
macro_rules! call {
    ($object:expr, $interface:ident::$method:ident $(, $arg:expr)* $(,)?) => {{
        let this = $object.as_raw().cast::<$interface>();
        ((*(*this).vtbl).$method)(this.cast() $(, $arg)*)
    }};
}
pub(crate) use call;

/// Calls a function that returns an interface through its out parameter.
pub(crate) fn out(f: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Result<ComPtr> {
    // SAFETY: The caller supplies a COM out parameter returning an IUnknown-compatible interface.
    unsafe { ComPtr::create(f) }
}

/// Associates an interface's ABI type with its IID.
///
/// # Safety
/// Implementors must represent the interface identified by IID with an IUnknown-compatible vtable.
pub(super) unsafe trait ComInterface {
    const IID: GUID;
}

/// One owned COM reference. IUnknown is the common type of heterogeneous XAML objects.
#[repr(transparent)]
pub(crate) struct ComPtr<T = IUnknown>(NonNull<T>);

impl<T> ComPtr<T> {
    /// The function must return an owned COM reference of type T.
    pub(super) unsafe fn create(f: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Result<Self> {
        let mut raw = null_mut();
        let result = f(&mut raw);
        // SAFETY: A non-null out parameter transfers an owned reference, including a partial result.
        let object = unsafe { Self::from_interface(raw.cast()) };
        check(result)?;
        object.ok_or(E_POINTER)
    }

    /// Takes ownership of one reference to a live COM interface of type T.
    pub(super) unsafe fn from_interface(raw: *mut T) -> Option<Self> {
        NonNull::new(raw).map(Self)
    }

    pub(crate) const fn as_raw(&self) -> *mut c_void {
        self.0.as_ptr().cast()
    }

    fn unknown(&self) -> &IUnknownVtbl {
        // SAFETY: Every live interface starts with an IUnknown-compatible vtable.
        unsafe { &*(*self.as_raw().cast::<IUnknown>()).vtbl }
    }

    /// Transfers this owned reference to the COM caller.
    #[cfg(test)]
    const fn into_raw(self) -> *mut c_void {
        let raw = self.as_raw();
        forget(self);
        raw
    }

    pub(crate) fn cast(&self, iid: &GUID) -> Result<ComPtr> {
        let raw = self.query_raw(iid)?;
        // SAFETY: Every queried interface can be held through its IUnknown prefix.
        Ok(ComPtr(raw.cast()))
    }

    pub(super) fn query<U: ComInterface>(&self) -> Result<ComPtr<U>> {
        let raw = self.query_raw(&U::IID)?;
        // U associates the requested IID with its ABI type.
        Ok(ComPtr(raw.cast()))
    }

    fn query_raw(&self, iid: &GUID) -> Result<NonNull<c_void>> {
        let mut raw = null_mut();
        // SAFETY: The source interface is live and raw is writable.
        check(unsafe { (self.unknown().QueryInterface)(self.as_raw(), iid, &mut raw) })?;
        NonNull::new(raw).ok_or(E_POINTER)
    }
}

impl ComPtr {
    /// Takes ownership of one reference through the common IUnknown prefix.
    pub(crate) unsafe fn from_raw(raw: *mut c_void) -> Option<Self> {
        // SAFETY: The caller transfers a reference with an IUnknown prefix.
        unsafe { Self::from_interface(raw.cast()) }
    }

    /// Borrows an interface through its IUnknown prefix without changing its reference count.
    ///
    /// # Safety
    /// A non-null pointer must remain a live COM interface on this apartment for the borrow.
    pub(super) const unsafe fn from_raw_borrowed(raw: &*mut c_void) -> Option<&Self> {
        if raw.is_null() {
            return None;
        }
        // SAFETY: ComPtr is transparent over a non-null pointer, and the caller keeps it live.
        Some(unsafe { &*from_ref(raw).cast::<Self>() })
    }
}

impl<T> Clone for ComPtr<T> {
    fn clone(&self) -> Self {
        // SAFETY: This reference keeps the object alive during AddRef.
        unsafe { (self.unknown().AddRef)(self.as_raw()) };
        Self(self.0)
    }
}

impl<T> Drop for ComPtr<T> {
    fn drop(&mut self) {
        // SAFETY: This pointer owns the reference it releases.
        unsafe { (self.unknown().Release)(self.as_raw()) };
    }
}

/// An owned WinRT string.
pub(crate) struct HString(HSTRING);

impl HString {
    pub(crate) fn new(text: &str) -> Self {
        Self::from_wide(&wide(text))
    }

    /// Copies a null-terminated UTF-16 string into an owned HSTRING.
    pub(crate) fn from_wide(text: &[u16]) -> Self {
        assert_eq!(text.last(), Some(&0));
        let mut string = null_mut();
        // SAFETY: The string is copied, an empty one is the null string.
        unsafe { WindowsCreateString(text.as_ptr(), (text.len() - 1) as u32, &mut string) };
        Self(string)
    }

    /// Takes over an owned string.
    pub(crate) const unsafe fn from_raw(string: HSTRING) -> Self {
        Self(string)
    }

    pub(crate) const fn as_raw(&self) -> HSTRING {
        self.0
    }

    /// Transfers the owned HSTRING to the COM caller.
    const fn into_raw(self) -> HSTRING {
        let raw = self.as_raw();
        forget(self);
        raw
    }

    pub(crate) fn to_string_lossy(&self) -> String {
        let mut length = 0;
        // SAFETY: The buffer lives as long as the string.
        unsafe {
            let buffer = WindowsGetStringRawBuffer(self.0, &mut length);
            if buffer.is_null() {
                return String::new();
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(buffer, length as usize))
        }
    }
}

impl Drop for HString {
    fn drop(&mut self) {
        // SAFETY: The string is owned, deleting the null string does nothing.
        unsafe { WindowsDeleteString(self.0) };
    }
}

/// Returns an activation factory or statics interface of a runtime class.
pub(crate) fn factory(class: &[u16], iid: &GUID) -> Result<ComPtr> {
    let class = HString::from_wide(class);
    // SAFETY: The class name and interface id are valid.
    out(|raw| unsafe { RoGetActivationFactory(class.as_raw(), iid, raw) })
}

/// Creates a runtime class with its default constructor.
pub(crate) fn activate(class: &[u16]) -> Result<ComPtr> {
    let class = HString::from_wide(class);
    // SAFETY: The class name is valid.
    out(|raw| unsafe { RoActivateInstance(class.as_raw(), raw) })
}

/// Creates a composable runtime class without extending it.
pub(crate) fn compose(class: &[u16], factory_iid: &GUID) -> Result<ComPtr> {
    let factory = factory(class, factory_iid)?;
    let mut inner = null_mut();
    // SAFETY: The IID selects the composable factory, and both outputs are writable.
    // A null outer requests an instance without aggregation.
    let value = out(|raw| unsafe {
        call!(
            factory,
            IComposableFactory::CreateInstance,
            null_mut(),
            &mut inner,
            raw
        )
    });
    // SAFETY: Without an outer object there is no inner reference to keep.
    drop(unsafe { ComPtr::from_raw(inner) });
    value
}

// MARK: COM objects

type QueryInterfaceFn =
    unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
type RefCountFn = unsafe extern "system" fn(*mut c_void) -> u32;
type GetIidsFn = unsafe extern "system" fn(*mut c_void, *mut u32, *mut *mut GUID) -> HRESULT;
type GetRuntimeClassNameFn = unsafe extern "system" fn(*mut c_void, *mut HSTRING) -> HRESULT;
type GetTrustLevelFn = unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT;

#[repr(C)]
pub(crate) struct IUnknown {
    vtbl: *const IUnknownVtbl,
}

#[repr(C)]
pub(super) struct IUnknownVtbl {
    pub(super) QueryInterface: QueryInterfaceFn,
    pub(super) AddRef: RefCountFn,
    pub(super) Release: RefCountFn,
}

#[repr(C)]
pub(super) struct IInspectableVtbl {
    base: IUnknownVtbl,
    GetIids: GetIidsFn,
    GetRuntimeClassName: GetRuntimeClassNameFn,
    GetTrustLevel: GetTrustLevelFn,
}

// Returns `this` for the given interfaces, which every object here also is agile for.
unsafe fn query_interface(
    this: *mut c_void,
    iid: *const GUID,
    object: *mut *mut c_void,
    interfaces: &[GUID],
    add_ref: RefCountFn,
) -> HRESULT {
    // SAFETY: COM passes a valid interface id and out pointer.
    unsafe {
        let iid = &*iid;
        if *iid == IID_IUNKNOWN || *iid == IID_IAGILE_OBJECT || interfaces.contains(iid) {
            add_ref(this);
            *object = this;
            S_OK
        } else {
            *object = null_mut();
            E_NOINTERFACE
        }
    }
}

unsafe extern "system" fn no_iids(
    _: *mut c_void,
    count: *mut u32,
    iids: *mut *mut GUID,
) -> HRESULT {
    // SAFETY: COM passes valid out pointers.
    unsafe {
        *count = 0;
        *iids = null_mut();
    }
    S_OK
}

unsafe extern "system" fn no_class_name(_: *mut c_void, name: *mut HSTRING) -> HRESULT {
    // SAFETY: COM passes a valid out pointer.
    unsafe { *name = null_mut() };
    S_OK
}

unsafe extern "system" fn base_trust(_: *mut c_void, level: *mut i32) -> HRESULT {
    // SAFETY: COM passes a valid out pointer.
    unsafe { *level = 0 };
    S_OK
}

// MARK: Event handlers

#[repr(C)]
struct EventHandlerVtbl {
    base: IUnknownVtbl,
    Invoke: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
}

// Called with the sender and the arguments of an event.
type EventCallback = dyn Fn(&ComPtr, Option<&ComPtr>);

#[repr(C)]
struct EventHandler {
    vtbl: &'static EventHandlerVtbl,
    refs: AtomicU32,
    iid: GUID,
    callback: Box<EventCallback>,
}

static EVENT_HANDLER_VTBL: EventHandlerVtbl = EventHandlerVtbl {
    base: IUnknownVtbl {
        QueryInterface: event_handler_query_interface,
        AddRef: event_handler_add_ref,
        Release: event_handler_release,
    },
    Invoke: event_handler_invoke,
};

/// Creates an event handler delegate with the interface `iid` and `(sender, args)` parameters,
/// like `Microsoft.UI.Xaml.RoutedEventHandler`, that calls `callback` with the sender and the
/// event arguments.
pub(crate) fn event_handler(
    iid: GUID,
    callback: impl Fn(&ComPtr, Option<&ComPtr>) + 'static,
) -> ComPtr {
    let handler = Box::new(EventHandler {
        vtbl: &EVENT_HANDLER_VTBL,
        refs: AtomicU32::new(1),
        iid,
        callback: Box::new(callback),
    });
    // SAFETY: The handler starts with one owned reference.
    unsafe { ComPtr::from_raw(Box::into_raw(handler).cast()) }.expect("handler")
}

unsafe extern "system" fn event_handler_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: Forwarded from COM.
    unsafe {
        query_interface(
            this,
            iid,
            object,
            &[(*this.cast::<EventHandler>()).iid],
            event_handler_add_ref,
        )
    }
}

unsafe extern "system" fn event_handler_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live handler.
    unsafe { &*this.cast::<EventHandler>() }
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn event_handler_release(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live handler, it is freed with its last reference.
    unsafe {
        let refs = (*this.cast::<EventHandler>())
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            - 1;
        if refs == 0 {
            drop(Box::from_raw(this.cast::<EventHandler>()));
        }
        refs
    }
}

unsafe extern "system" fn event_handler_invoke(
    this: *mut c_void,
    sender: *mut c_void,
    args: *mut c_void,
) -> HRESULT {
    // SAFETY: `this` is a live handler, the sender and arguments are borrowed for the call.
    unsafe {
        if let Some(sender) = ComPtr::from_raw_borrowed(&sender) {
            let args = ComPtr::from_raw_borrowed(&args);
            ((*this.cast::<EventHandler>()).callback)(sender, args);
        }
    }
    S_OK
}

// MARK: IReference<Color>

#[repr(C)]
struct ColorReferenceVtbl {
    base: IInspectableVtbl,
    get_Value: unsafe extern "system" fn(*mut c_void, *mut Color) -> HRESULT,
}

#[repr(C)]
struct ColorReference {
    vtbl: &'static ColorReferenceVtbl,
    refs: AtomicU32,
    color: Color,
}

static COLOR_REFERENCE_VTBL: ColorReferenceVtbl = ColorReferenceVtbl {
    base: IInspectableVtbl {
        base: IUnknownVtbl {
            QueryInterface: color_reference_query_interface,
            AddRef: color_reference_add_ref,
            Release: color_reference_release,
        },
        GetIids: no_iids,
        GetRuntimeClassName: no_class_name,
        GetTrustLevel: base_trust,
    },
    get_Value: color_reference_value,
};

/// Boxes a color as an `IReference<Windows.UI.Color>`.
pub(crate) fn color_reference(color: Color) -> ComPtr {
    let reference = Box::new(ColorReference {
        vtbl: &COLOR_REFERENCE_VTBL,
        refs: AtomicU32::new(1),
        color,
    });
    // SAFETY: The reference starts with one owned reference.
    unsafe { ComPtr::from_raw(Box::into_raw(reference).cast()) }.expect("color")
}

unsafe extern "system" fn color_reference_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: Forwarded from COM.
    unsafe {
        query_interface(
            this,
            iid,
            object,
            &[IID_IINSPECTABLE, IID_IREFERENCE_COLOR],
            color_reference_add_ref,
        )
    }
}

unsafe extern "system" fn color_reference_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live reference.
    unsafe { &*this.cast::<ColorReference>() }
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn color_reference_release(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live reference, it is freed with its last reference.
    unsafe {
        let refs = (*this.cast::<ColorReference>())
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            - 1;
        if refs == 0 {
            drop(Box::from_raw(this.cast::<ColorReference>()));
        }
        refs
    }
}

unsafe extern "system" fn color_reference_value(this: *mut c_void, value: *mut Color) -> HRESULT {
    // SAFETY: `this` is a live reference and COM passes a valid out pointer.
    unsafe { *value = (*this.cast::<ColorReference>()).color };
    S_OK
}

// MARK: Application

#[repr(C)]
struct ApplicationOverridesVtbl {
    base: IInspectableVtbl,
    OnLaunched: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IXamlMetadataProvider {
    vtbl: *const XamlMetadataProviderVtbl,
}

#[repr(C)]
struct XamlMetadataProviderVtbl {
    base: IInspectableVtbl,
    GetXamlType: unsafe extern "system" fn(*mut c_void, TypeName, *mut *mut c_void) -> HRESULT,
    GetXamlTypeByFullName:
        unsafe extern "system" fn(*mut c_void, HSTRING, *mut *mut c_void) -> HRESULT,
    GetXmlnsDefinitions:
        unsafe extern "system" fn(*mut c_void, *mut u32, *mut *mut c_void) -> HRESULT,
}

/// The outer object of the XAML `Application`, like the `App` class of a WinUI app. It gives
/// XAML the type information of the WinUI controls library, so their styles and templates load.
#[repr(C)]
struct XamlApplication {
    overrides: &'static ApplicationOverridesVtbl,
    provider: &'static XamlMetadataProviderVtbl,
    refs: AtomicU32,
    // The non-delegating inner object of the composed Application.
    inner: Cell<*mut c_void>,
    controls: OnceCell<Option<ComPtr>>,
}

static APPLICATION_OVERRIDES_VTBL: ApplicationOverridesVtbl = ApplicationOverridesVtbl {
    base: IInspectableVtbl {
        base: IUnknownVtbl {
            QueryInterface: application_query_interface,
            AddRef: application_add_ref,
            Release: application_release,
        },
        GetIids: application_get_iids,
        GetRuntimeClassName: application_get_runtime_class_name,
        GetTrustLevel: base_trust,
    },
    OnLaunched: application_on_launched,
};

static XAML_METADATA_PROVIDER_VTBL: XamlMetadataProviderVtbl = XamlMetadataProviderVtbl {
    base: IInspectableVtbl {
        base: IUnknownVtbl {
            QueryInterface: provider_query_interface,
            AddRef: provider_add_ref,
            Release: provider_release,
        },
        GetIids: application_get_iids,
        GetRuntimeClassName: application_get_runtime_class_name,
        GetTrustLevel: base_trust,
    },
    GetXamlType: provider_get_xaml_type,
    GetXamlTypeByFullName: provider_get_xaml_type_by_full_name,
    GetXmlnsDefinitions: provider_get_xmlns_definitions,
};

/// Creates the `Microsoft.UI.Xaml.Application` of this thread and returns its `IApplication`.
pub(crate) fn create_application() -> Result<ComPtr> {
    let factory = factory(
        w!("Microsoft.UI.Xaml.Application"),
        &IID_IAPPLICATION_FACTORY,
    )?;
    compose_application(|outer, inner, value| {
        // SAFETY: The caller retains the outer object through composition and supplies
        // writable pointers for both owned outputs.
        unsafe {
            call!(
                factory,
                IComposableFactory::CreateInstance,
                outer,
                inner,
                value
            )
        }
    })
}

fn compose_application(
    create: impl FnOnce(*mut c_void, *mut *mut c_void, *mut *mut c_void) -> HRESULT,
) -> Result<ComPtr> {
    let outer = Box::into_raw(Box::new(XamlApplication {
        overrides: &APPLICATION_OVERRIDES_VTBL,
        provider: &XAML_METADATA_PROVIDER_VTBL,
        refs: AtomicU32::new(1),
        inner: Cell::new(null_mut()),
        controls: OnceCell::new(),
    }));
    // SAFETY: The new outer object owns its initial COM reference. Keep that reference
    // guarded even if the factory fails after temporarily retaining the object.
    let _outer = unsafe { ComPtr::from_raw(outer.cast()) }.expect("application outer");
    let mut inner = null_mut();
    let application = out(|value| create(outer.cast(), &mut inner, value));
    // SAFETY: The guard keeps outer alive, and its destructor will release any returned inner.
    unsafe { (*outer).inner.set(inner) };
    application
}

const unsafe fn application_from_provider(this: *mut c_void) -> *mut c_void {
    // SAFETY: `this` points to the provider vtable field of an application.
    unsafe { this.byte_sub(offset_of!(XamlApplication, provider)) }
}

unsafe extern "system" fn application_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: `this` is a live application and COM passes valid pointers.
    unsafe {
        let application = &*this.cast::<XamlApplication>();
        let requested = &*iid;
        if *requested == IID_IXAML_METADATA_PROVIDER {
            application_add_ref(this);
            *object = this.byte_add(offset_of!(XamlApplication, provider));
            return S_OK;
        }
        if [IID_IUNKNOWN, IID_IINSPECTABLE, IID_IAPPLICATION_OVERRIDES].contains(requested) {
            application_add_ref(this);
            *object = this;
            return S_OK;
        }
        // Everything else is the Application itself.
        let inner = application.inner.get();
        match ComPtr::from_raw_borrowed(&inner) {
            Some(inner) => call!(inner, IUnknown::QueryInterface, iid, object),
            None => {
                *object = null_mut();
                E_NOINTERFACE
            }
        }
    }
}

unsafe extern "system" fn application_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live application.
    unsafe { &*this.cast::<XamlApplication>() }
        .refs
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

unsafe extern "system" fn application_release(this: *mut c_void) -> u32 {
    // SAFETY: `this` is a live application, it releases its inner object and is freed with
    // its last reference.
    unsafe {
        let refs = (*this.cast::<XamlApplication>())
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            - 1;
        if refs == 0 {
            let application = Box::from_raw(this.cast::<XamlApplication>());
            drop(ComPtr::from_raw(application.inner.get()));
        }
        refs
    }
}

unsafe extern "system" fn application_get_iids(
    this: *mut c_void,
    count: *mut u32,
    iids: *mut *mut GUID,
) -> HRESULT {
    // SAFETY: Forwarded from COM.
    unsafe { no_iids(this, count, iids) }
}

unsafe extern "system" fn application_get_runtime_class_name(
    _this: *mut c_void,
    name: *mut HSTRING,
) -> HRESULT {
    // SAFETY: COM passes a valid out pointer, the caller owns the new string.
    unsafe { *name = HString::from_wide(w!("Microsoft.UI.Xaml.Application")).into_raw() };
    S_OK
}

const unsafe extern "system" fn application_on_launched(
    _this: *mut c_void,
    _args: *mut c_void,
) -> HRESULT {
    // Islands are never launched.
    S_OK
}

unsafe extern "system" fn provider_query_interface(
    this: *mut c_void,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: Forwarded to the application owning the provider.
    unsafe { application_query_interface(application_from_provider(this), iid, object) }
}

unsafe extern "system" fn provider_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: Forwarded to the application owning the provider.
    unsafe { application_add_ref(application_from_provider(this)) }
}

unsafe extern "system" fn provider_release(this: *mut c_void) -> u32 {
    // SAFETY: Forwarded to the application owning the provider.
    unsafe { application_release(application_from_provider(this)) }
}

// The metadata provider of the WinUI controls library, created when XAML first needs it.
unsafe fn controls_provider(this: *mut c_void) -> Option<&'static ComPtr> {
    // SAFETY: The application lives as long as XAML does on this thread.
    let application = unsafe { &*application_from_provider(this).cast::<XamlApplication>() };
    application
        .controls
        .get_or_init(|| {
            activate(w!(
                "Microsoft.UI.Xaml.XamlTypeInfo.XamlControlsXamlMetaDataProvider"
            ))
            .and_then(|provider| provider.cast(&IID_IXAML_METADATA_PROVIDER))
            .ok()
        })
        .as_ref()
}

unsafe extern "system" fn provider_get_xaml_type(
    this: *mut c_void,
    type_name: TypeName,
    result: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: COM supplies a writable output, and the provider owns IXamlMetadataProvider.
    unsafe {
        *result = null_mut();
        match controls_provider(this) {
            Some(provider) => call!(
                provider,
                IXamlMetadataProvider::GetXamlType,
                type_name,
                result
            ),
            None => S_OK,
        }
    }
}

unsafe extern "system" fn provider_get_xaml_type_by_full_name(
    this: *mut c_void,
    full_name: HSTRING,
    result: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: COM supplies a live name and a writable output. The provider owns IXamlMetadataProvider.
    unsafe {
        *result = null_mut();
        match controls_provider(this) {
            Some(provider) => call!(
                provider,
                IXamlMetadataProvider::GetXamlTypeByFullName,
                full_name,
                result
            ),
            None => S_OK,
        }
    }
}

unsafe extern "system" fn provider_get_xmlns_definitions(
    this: *mut c_void,
    count: *mut u32,
    definitions: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: COM supplies writable count and array outputs, and the provider owns IXamlMetadataProvider.
    unsafe {
        *count = 0;
        *definitions = null_mut();
        match controls_provider(this) {
            Some(provider) => {
                call!(
                    provider,
                    IXamlMetadataProvider::GetXmlnsDefinitions,
                    count,
                    definitions
                )
            }
            None => S_OK,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;

    #[test]
    fn callback_borrows_arguments_without_changing_refcounts() {
        let sender = event_handler(IID_ROUTED_EVENT_HANDLER, |_, _| {});
        let args = event_handler(IID_ROUTED_EVENT_HANDLER, |_, _| {});
        let invoked = Rc::new(Cell::new(false));
        let handler = event_handler(IID_ROUTED_EVENT_HANDLER, {
            let invoked = invoked.clone();
            move |sender, args| {
                for object in [sender, args.expect("event args")] {
                    // SAFETY: Both pointers were supplied by live EventHandler test objects.
                    let object = unsafe { &*object.as_raw().cast::<EventHandler>() };
                    assert_eq!(object.refs.load(Ordering::Relaxed), 1);
                }
                invoked.set(true);
            }
        });
        // SAFETY: All three event objects are live for the duration of this call.
        let result =
            unsafe { event_handler_invoke(handler.as_raw(), sender.as_raw(), args.as_raw()) };
        assert_eq!(result, S_OK);
        assert!(invoked.get());
    }

    #[test]
    fn failed_interface_call_releases_a_partial_result() {
        let marker = Rc::new(());
        let weak = Rc::downgrade(&marker);
        let result = out(move |value| {
            let object = event_handler(IID_ROUTED_EVENT_HANDLER, move |_, _| {
                let _ = &marker;
            });
            // SAFETY: The caller supplies a writable output for this owned interface.
            unsafe { *value = object.into_raw() };
            E_NOINTERFACE
        });
        assert!(result.is_err());
        assert!(
            weak.upgrade().is_none(),
            "failed call retained its partial result"
        );
    }

    #[test]
    fn application_composition_balances_outer_and_inner_references() {
        for status in [S_OK, E_NOINTERFACE] {
            let marker = Rc::new(());
            let weak = Rc::downgrade(&marker);
            let result = compose_application(move |outer, inner, value| {
                let controls_marker = marker.clone();
                let controls = event_handler(IID_ROUTED_EVENT_HANDLER, move |_, _| {
                    let _ = &controls_marker;
                });
                let inner_object = event_handler(IID_ROUTED_EVENT_HANDLER, move |_, _| {
                    let _ = &marker;
                });
                // SAFETY: Composition supplies writable outputs and borrows the live outer.
                unsafe {
                    *inner = inner_object.into_raw();
                    let application = &*outer.cast::<XamlApplication>();
                    assert!(application.controls.set(Some(controls)).is_ok());
                    application_add_ref(outer);
                    application_release(outer);
                    if status == S_OK {
                        application_add_ref(outer);
                        *value = outer;
                    }
                }
                status
            });
            match result {
                Ok(application) => {
                    assert!(
                        weak.upgrade().is_some(),
                        "composition released its state early"
                    );
                    drop(application);
                }
                Err(error) => assert_eq!(error, status),
            }
            assert!(weak.upgrade().is_none(), "composition retained its state");
        }
    }
}

interface! {
    IComposableFactory, IComposableFactoryVtbl: IInspectableVtbl {
        CreateInstance: unsafe extern "system" fn(*mut IComposableFactory, *mut c_void, *mut *mut c_void, *mut *mut c_void) -> HRESULT,
    }
}
