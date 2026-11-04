/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Handwritten WinRT bindings for the Windows App SDK: reference counted interface pointers,
//! strings, activation and the COM objects ImageView implements itself. Methods are called by
//! their vtable slot as listed in the Windows App SDK metadata, after the six `IInspectable`
//! slots.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::mem::{self, ManuallyDrop, offset_of};
use std::ptr::{NonNull, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) type HRESULT = i32;
pub(crate) type HSTRING = *mut c_void;
pub(crate) type Result<T> = std::result::Result<T, HRESULT>;

const S_OK: HRESULT = 0;
const E_NOINTERFACE: HRESULT = 0x8000_4002_u32 as i32;
const E_POINTER: HRESULT = 0x8000_4003_u32 as i32;

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
pub(crate) const IID_KEY_EVENT_HANDLER: GUID =
    GUID::from_u128(0xdb68e7cc_9a2b_527d_9989_25284daccc03);
pub(crate) const IID_IKEY_ROUTED_EVENT_ARGS: GUID =
    GUID::from_u128(0xee357007_a2d6_5c75_9431_05fd66ec7915);
pub(crate) const IID_ICONTROL: GUID = GUID::from_u128(0x857d6e8a_d45a_5c69_a99c_bf6a5c54fb38);
pub(crate) const IID_ITOGGLE_BUTTON: GUID = GUID::from_u128(0x686fbaa4_c866_568b_8f75_481d8d545291);
pub(crate) const IID_IRANGE_BASE: GUID = GUID::from_u128(0x540d6d61_8fac_5d5c_b5b0_e172a7dde103);
pub(crate) const IID_ISELECTOR: GUID = GUID::from_u128(0x8f7e2159_e61d_576f_8476_f83fde3d689e);
pub(crate) const IID_INUMBER_BOX: GUID = GUID::from_u128(0xc18eb0e9_29fb_525d_abbc_d6b2110f542e);
pub(crate) const IID_IBORDER: GUID = GUID::from_u128(0x1ca13b47_ff5c_5abc_a411_a177df9482a9);
pub(crate) const IID_IMENU_FLYOUT_ITEM: GUID =
    GUID::from_u128(0x4252df5a_44f9_5ee8_b1cc_53de9aaa4095);
pub(crate) const IID_RANGE_BASE_VALUE_CHANGED_EVENT_HANDLER: GUID =
    GUID::from_u128(0x23f0e209_9455_54cb_b8bc_0b49553c7dcc);
pub(crate) const IID_SELECTION_CHANGED_EVENT_HANDLER: GUID =
    GUID::from_u128(0xa232390d_0e34_595e_8931_fa928a9909f4);
// TypedEventHandler<NumberBox, NumberBoxValueChangedEventArgs>, derived from its parameterized type
// signature.
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

/// Calls the method in vtable `slot` of an interface with the given arguments and types,
/// which must be the parameters of that method.
macro_rules! call {
    ($object:expr, $slot:expr $(, $arg:expr => $ty:ty)* $(,)?) => {
        $object.method::<unsafe extern "system" fn(
            *mut std::ffi::c_void $(, $ty)*
        ) -> $crate::platforms::windows::winrt::HRESULT>($slot)(
            $object.as_raw() $(, $arg)*
        )
    };
}
pub(crate) use call;

/// Calls a function that returns an interface through its out parameter.
pub(crate) fn out(f: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Result<ComPtr> {
    let mut raw = null_mut();
    check(f(&mut raw))?;
    // SAFETY: A succeeded call returns an owned reference.
    unsafe { ComPtr::from_raw(raw) }.ok_or(E_POINTER)
}

/// An owned reference to a COM interface.
pub(crate) struct ComPtr(NonNull<c_void>);

impl ComPtr {
    /// Takes over an owned reference.
    pub(crate) unsafe fn from_raw(raw: *mut c_void) -> Option<Self> {
        NonNull::new(raw).map(Self)
    }

    pub(crate) const fn as_raw(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    /// Returns the method in vtable `slot`.
    ///
    /// # Safety
    /// `F` must be the function pointer type of the method in that slot.
    pub(crate) unsafe fn method<F: Copy>(&self, slot: usize) -> F {
        debug_assert_eq!(size_of::<F>(), size_of::<usize>());
        // SAFETY: Every COM interface starts with a pointer to its vtable.
        unsafe {
            let vtable = *self.0.as_ptr().cast::<*const usize>();
            mem::transmute_copy(&*vtable.add(slot))
        }
    }

    pub(crate) fn cast(&self, iid: &GUID) -> Result<Self> {
        // SAFETY: Slot 0 is IUnknown::QueryInterface.
        out(|raw| unsafe { call!(self, 0, iid => *const GUID, raw => *mut *mut c_void) })
    }

    /// Calls a property getter that returns an interface.
    pub(crate) unsafe fn get(&self, slot: usize) -> Result<Self> {
        // SAFETY: The caller passes the slot of a getter returning an interface.
        out(|raw| unsafe { call!(self, slot, raw => *mut *mut c_void) })
    }

    /// Calls a property setter that takes an interface or a string.
    pub(crate) unsafe fn put(&self, slot: usize, value: *mut c_void) -> Result<()> {
        // SAFETY: The caller passes the slot of a setter taking a pointer.
        check(unsafe { call!(self, slot, value => *mut c_void) })
    }
}

impl Clone for ComPtr {
    fn clone(&self) -> Self {
        // SAFETY: Slot 1 is IUnknown::AddRef.
        unsafe { self.method::<unsafe extern "system" fn(*mut c_void) -> u32>(1)(self.as_raw()) };
        Self(self.0)
    }
}

impl Drop for ComPtr {
    fn drop(&mut self) {
        // SAFETY: Slot 2 is IUnknown::Release and this reference is owned.
        unsafe { self.method::<unsafe extern "system" fn(*mut c_void) -> u32>(2)(self.as_raw()) };
    }
}

/// An owned WinRT string.
pub(crate) struct HString(HSTRING);

impl HString {
    pub(crate) fn new(text: &str) -> Self {
        let text = text.encode_utf16().collect::<Vec<_>>();
        let mut string = null_mut();
        // SAFETY: The string is copied, an empty one is the null string.
        unsafe { WindowsCreateString(text.as_ptr(), text.len() as u32, &mut string) };
        Self(string)
    }

    /// Takes over an owned string.
    pub(crate) const unsafe fn from_raw(string: HSTRING) -> Self {
        Self(string)
    }

    pub(crate) const fn as_raw(&self) -> HSTRING {
        self.0
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
pub(crate) fn factory(class: &str, iid: &GUID) -> Result<ComPtr> {
    let class = HString::new(class);
    // SAFETY: The class name and interface id are valid.
    out(|raw| unsafe { RoGetActivationFactory(class.as_raw(), iid, raw) })
}

/// Creates a runtime class with its default constructor.
pub(crate) fn activate(class: &str) -> Result<ComPtr> {
    let class = HString::new(class);
    // SAFETY: The class name is valid.
    out(|raw| unsafe { RoActivateInstance(class.as_raw(), raw) })
}

/// Creates a composable runtime class without extending it.
pub(crate) fn compose(class: &str, factory_iid: &GUID) -> Result<ComPtr> {
    let factory = factory(class, factory_iid)?;
    let mut inner = null_mut();
    // SAFETY: Slot 6 of a composable factory is CreateInstance(outer, inner, value).
    let value = out(|raw| unsafe {
        call!(factory, 6, null_mut() => *mut c_void, &mut inner => *mut *mut c_void, raw => *mut *mut c_void)
    })?;
    // SAFETY: Without an outer object there is no inner reference to keep.
    drop(unsafe { ComPtr::from_raw(inner) });
    Ok(value)
}

// MARK: COM objects

type QueryInterfaceFn =
    unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
type RefCountFn = unsafe extern "system" fn(*mut c_void) -> u32;
type GetIidsFn = unsafe extern "system" fn(*mut c_void, *mut u32, *mut *mut GUID) -> HRESULT;
type GetRuntimeClassNameFn = unsafe extern "system" fn(*mut c_void, *mut HSTRING) -> HRESULT;
type GetTrustLevelFn = unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT;

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
    QueryInterface: QueryInterfaceFn,
    AddRef: RefCountFn,
    Release: RefCountFn,
    Invoke: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
}

// Called with the sender and the arguments of an event.
type EventCallback = dyn Fn(&ComPtr, Option<&ComPtr>);

#[repr(C)]
struct EventHandler {
    vtable: &'static EventHandlerVtbl,
    refs: AtomicU32,
    iid: GUID,
    callback: Box<EventCallback>,
}

static EVENT_HANDLER_VTBL: EventHandlerVtbl = EventHandlerVtbl {
    QueryInterface: event_handler_query_interface,
    AddRef: event_handler_add_ref,
    Release: event_handler_release,
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
        vtable: &EVENT_HANDLER_VTBL,
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
        if let Some(sender) = ComPtr::from_raw(sender) {
            let sender = ManuallyDrop::new(sender);
            let args = ComPtr::from_raw(args).map(ManuallyDrop::new);
            ((*this.cast::<EventHandler>()).callback)(&sender, args.as_deref());
        }
    }
    S_OK
}

// MARK: IReference<Color>

#[repr(C)]
struct ColorReferenceVtbl {
    QueryInterface: QueryInterfaceFn,
    AddRef: RefCountFn,
    Release: RefCountFn,
    GetIids: GetIidsFn,
    GetRuntimeClassName: GetRuntimeClassNameFn,
    GetTrustLevel: GetTrustLevelFn,
    get_Value: unsafe extern "system" fn(*mut c_void, *mut Color) -> HRESULT,
}

#[repr(C)]
struct ColorReference {
    vtable: &'static ColorReferenceVtbl,
    refs: AtomicU32,
    color: Color,
}

static COLOR_REFERENCE_VTBL: ColorReferenceVtbl = ColorReferenceVtbl {
    QueryInterface: color_reference_query_interface,
    AddRef: color_reference_add_ref,
    Release: color_reference_release,
    GetIids: no_iids,
    GetRuntimeClassName: no_class_name,
    GetTrustLevel: base_trust,
    get_Value: color_reference_value,
};

/// Boxes a color as an `IReference<Windows.UI.Color>`.
pub(crate) fn color_reference(color: Color) -> ComPtr {
    let reference = Box::new(ColorReference {
        vtable: &COLOR_REFERENCE_VTBL,
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
    QueryInterface: QueryInterfaceFn,
    AddRef: RefCountFn,
    Release: RefCountFn,
    GetIids: GetIidsFn,
    GetRuntimeClassName: GetRuntimeClassNameFn,
    GetTrustLevel: GetTrustLevelFn,
    OnLaunched: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
}

#[repr(C)]
struct XamlMetadataProviderVtbl {
    QueryInterface: QueryInterfaceFn,
    AddRef: RefCountFn,
    Release: RefCountFn,
    GetIids: GetIidsFn,
    GetRuntimeClassName: GetRuntimeClassNameFn,
    GetTrustLevel: GetTrustLevelFn,
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
    QueryInterface: application_query_interface,
    AddRef: application_add_ref,
    Release: application_release,
    GetIids: application_get_iids,
    GetRuntimeClassName: application_get_runtime_class_name,
    GetTrustLevel: base_trust,
    OnLaunched: application_on_launched,
};

static XAML_METADATA_PROVIDER_VTBL: XamlMetadataProviderVtbl = XamlMetadataProviderVtbl {
    QueryInterface: provider_query_interface,
    AddRef: provider_add_ref,
    Release: provider_release,
    GetIids: application_get_iids,
    GetRuntimeClassName: application_get_runtime_class_name,
    GetTrustLevel: base_trust,
    GetXamlType: provider_get_xaml_type,
    GetXamlTypeByFullName: provider_get_xaml_type_by_full_name,
    GetXmlnsDefinitions: provider_get_xmlns_definitions,
};

/// Creates the `Microsoft.UI.Xaml.Application` of this thread and returns its `IApplication`.
pub(crate) fn create_application() -> Result<ComPtr> {
    let factory = factory("Microsoft.UI.Xaml.Application", &IID_IAPPLICATION_FACTORY)?;
    let outer = Box::into_raw(Box::new(XamlApplication {
        overrides: &APPLICATION_OVERRIDES_VTBL,
        provider: &XAML_METADATA_PROVIDER_VTBL,
        refs: AtomicU32::new(1),
        inner: Cell::new(null_mut()),
        controls: OnceCell::new(),
    }));
    let mut inner = null_mut();
    // SAFETY: Slot 6 is CreateInstance(outer, inner, value), the outer object stays alive
    // while the composed Application holds it.
    // A failed creation already released the outer object.
    let application = out(|raw| unsafe {
        call!(factory, 6, outer.cast::<c_void>() => *mut c_void, &mut inner => *mut *mut c_void, raw => *mut *mut c_void)
    })?;
    // SAFETY: The outer object is alive, its own reference is released after the Application
    // took one.
    unsafe {
        (*outer).inner.set(inner);
        application_release(outer.cast());
    }
    Ok(application)
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
        match ComPtr::from_raw(application.inner.get()) {
            Some(inner) => {
                let inner = ManuallyDrop::new(inner);
                call!(inner, 0, iid => *const GUID, object => *mut *mut c_void)
            }
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
    unsafe { *name = ManuallyDrop::new(HString::new("Microsoft.UI.Xaml.Application")).as_raw() };
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
            activate("Microsoft.UI.Xaml.XamlTypeInfo.XamlControlsXamlMetaDataProvider")
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
    // SAFETY: Forwarded to the same slot of the controls provider.
    unsafe {
        *result = null_mut();
        match controls_provider(this) {
            Some(provider) => call!(provider, 6, type_name => TypeName, result => *mut *mut c_void),
            None => S_OK,
        }
    }
}

unsafe extern "system" fn provider_get_xaml_type_by_full_name(
    this: *mut c_void,
    full_name: HSTRING,
    result: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: Forwarded to the same slot of the controls provider.
    unsafe {
        *result = null_mut();
        match controls_provider(this) {
            Some(provider) => call!(provider, 7, full_name => HSTRING, result => *mut *mut c_void),
            None => S_OK,
        }
    }
}

unsafe extern "system" fn provider_get_xmlns_definitions(
    this: *mut c_void,
    count: *mut u32,
    definitions: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: Forwarded to the same slot of the controls provider.
    unsafe {
        *count = 0;
        *definitions = null_mut();
        match controls_provider(this) {
            Some(provider) => {
                call!(provider, 8, count => *mut u32, definitions => *mut *mut c_void)
            }
            None => S_OK,
        }
    }
}
