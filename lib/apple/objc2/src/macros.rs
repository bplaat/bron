/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;

use crate::ffi::objc_msgSend;
use crate::runtime::{AnyObject, Sel};

/// Get class by name
#[macro_export]
macro_rules! class {
    ($name:ident) => {{
        #[allow(unused_unsafe)]
        // SAFETY: `name` is a compile-time null-terminated string literal. The returned
        // pointer is either a valid class pointer or null (caller is responsible for checking).
        unsafe {
            let name = concat!(stringify!($name), '\0');
            $crate::ffi::objc_getClass(name.as_ptr() as *const std::ffi::c_char)
                as *mut $crate::runtime::AnyObject
        }
    }};
}

/// Get selector by name
#[macro_export]
macro_rules! sel {
    ($name:ident) => {{
        #[allow(unused_unsafe)]
        // SAFETY: `name` is a compile-time null-terminated string literal.
        // `sel_registerName` always returns a valid, globally registered selector pointer.
        unsafe {
            let name = concat!(stringify!($name), '\0');
            $crate::runtime::Sel(
                $crate::ffi::sel_registerName(name.as_ptr() as *const std::ffi::c_char)
                    as *const std::ffi::c_void,
            )
        }
    }};
    ($($name:ident :)+) => ({
        #[allow(unused_unsafe)]
        // SAFETY: `name` is a compile-time null-terminated string literal.
        // `sel_registerName` always returns a valid, globally registered selector pointer.
        unsafe {
            let name = concat!($(stringify!($name), ':'),+, '\0');
            $crate::runtime::Sel(
                $crate::ffi::sel_registerName(name.as_ptr() as *const std::ffi::c_char)
                    as *const std::ffi::c_void,
            )
        }
    });
}

/// Internal trait used by `msg_send!`. Not part of the public API.
#[doc(hidden)]
pub trait MessageSend {
    /// # Safety
    /// Caller must ensure `obj` is a valid ObjC object and `sel` is a valid selector.
    unsafe fn invoke<R: crate::Encode>(obj: *mut AnyObject, sel: Sel, args: Self, owned: bool)
    -> R;

    /// # Safety
    /// Caller must ensure the receiver and superclass describe a valid super dispatch.
    unsafe fn invoke_super<R: crate::Encode>(
        receiver: *mut AnyObject,
        superclass: *const crate::runtime::AnyClass,
        sel: Sel,
        args: Self,
        owned: bool,
    ) -> R;
}
macro_rules! message_send_impl {
    ($($a:ident : $t:ident),*) => (
        impl<$($t: crate::Encode),*> MessageSend for ($($t,)*) {
            #[inline(always)]
            #[allow(clippy::undocumented_unsafe_blocks)]
            unsafe fn invoke<R: crate::Encode>(obj: *mut AnyObject, sel: Sel, ($($a,)*): Self, owned: bool) -> R {
                #[cfg(debug_assertions)]
                crate::verify::verify_send(obj, sel, &[$($t::ENCODING),*], &R::ENCODING);
                if R::IS_OBJECT_OWNERSHIP {
                    // SAFETY: Ownership wrappers use the Objective-C object pointer return ABI.
                    let imp: unsafe extern "C-unwind" fn(*mut AnyObject, *const c_void, $($t,)*) -> *mut c_void =
                        unsafe { std::mem::transmute(objc_msgSend as *const c_void) };
                    // SAFETY: The caller guarantees the receiver, selector and arguments match.
                    let pointer = unsafe { imp(obj, sel.0, $($a,)*) };
                    // SAFETY: The wrapper declared that it accepts an Objective-C object return.
                    return unsafe { R::from_object_return(pointer, owned) };
                }
                cfg_select! {
                    target_arch = "x86_64" => unsafe {
                        // SAFETY: `objc_msgSend`/`objc_msgSend_stret` are C variadics that accept any
                        // argument list. The transmute gives them the concrete Rust types we verified via
                        // `verify_send` (debug) or statically via `Encode` bounds (release). The call is
                        // sound because the caller (`msg_send!`) must ensure `obj` is a valid ObjC object
                        // and `sel` is a registered selector, as documented on `MessageSend::invoke`.
                        if const { size_of::<R>() > 16 } {
                            let mut ret = std::mem::zeroed();
                            let imp: unsafe extern "C-unwind" fn (*mut R, *mut AnyObject, *const c_void, $($t,)*) =
                                std::mem::transmute(crate::ffi::objc_msgSend_stret as *const c_void);
                            imp(&mut ret, obj, sel.0, $($a,)*);
                            ret
                        } else {
                            let imp: unsafe extern "C-unwind" fn (*mut AnyObject, *const c_void, $($t,)*) -> R =
                                std::mem::transmute(objc_msgSend as *const c_void);
                            imp(obj, sel.0, $($a,)*)
                        }
                    }
                    _ => unsafe {
                        // SAFETY: see the x86_64 branch above for the full justification.
                        let imp: unsafe extern "C-unwind" fn (*mut AnyObject, *const c_void, $($t,)*) -> R =
                            std::mem::transmute(objc_msgSend as *const c_void);
                        imp(obj, sel.0, $($a,)*)
                    }
                }
            }

            #[inline(always)]
            #[allow(clippy::undocumented_unsafe_blocks)]
            unsafe fn invoke_super<R: crate::Encode>(receiver: *mut AnyObject, superclass: *const crate::runtime::AnyClass, sel: Sel, ($($a,)*): Self, owned: bool) -> R {
                #[cfg(debug_assertions)]
                crate::verify::verify_super_send(
                    superclass,
                    sel,
                    &[$($t::ENCODING),*],
                    &R::ENCODING,
                );
                let super_info = crate::ffi::objc_super {
                    receiver,
                    super_class: superclass,
                };
                if R::IS_OBJECT_OWNERSHIP {
                    let imp: unsafe extern "C-unwind" fn(*const crate::ffi::objc_super, *const c_void, $($t,)*) -> *mut c_void =
                        unsafe { std::mem::transmute(crate::ffi::objc_msgSendSuper as *const c_void) };
                    let pointer = unsafe { imp(&super_info, sel.0, $($a,)*) };
                    return unsafe { R::from_object_return(pointer, owned) };
                }
                cfg_select! {
                    target_arch = "x86_64" => unsafe {
                        if const { size_of::<R>() > 16 } {
                            let mut ret = std::mem::zeroed();
                            let imp: unsafe extern "C-unwind" fn(*mut R, *const crate::ffi::objc_super, *const c_void, $($t,)*) =
                                std::mem::transmute(crate::ffi::objc_msgSendSuper_stret as *const c_void);
                            imp(&mut ret, &super_info, sel.0, $($a,)*);
                            ret
                        } else {
                            let imp: unsafe extern "C-unwind" fn(*const crate::ffi::objc_super, *const c_void, $($t,)*) -> R =
                                std::mem::transmute(crate::ffi::objc_msgSendSuper as *const c_void);
                            imp(&super_info, sel.0, $($a,)*)
                        }
                    }
                    _ => unsafe {
                        let imp: unsafe extern "C-unwind" fn(*const crate::ffi::objc_super, *const c_void, $($t,)*) -> R =
                            std::mem::transmute(crate::ffi::objc_msgSendSuper as *const c_void);
                        imp(&super_info, sel.0, $($a,)*)
                    }
                }
            }
        }
    );
}

/// Converts a receiver expression into the raw object pointer used by objc_msgSend.
#[doc(hidden)]
pub trait MessageReceiver {
    /// Converts the receiver, consuming ownership wrappers when required by initialization.
    fn into_raw(self) -> *mut AnyObject;
}

impl<T> MessageReceiver for *mut T {
    fn into_raw(self) -> *mut AnyObject {
        self.cast::<AnyObject>()
    }
}

impl<T> MessageReceiver for *const T {
    fn into_raw(self) -> *mut AnyObject {
        self.cast_mut().cast::<AnyObject>()
    }
}

impl<T: crate::runtime::Message> MessageReceiver for &T {
    fn into_raw(self) -> *mut AnyObject {
        (self as *const T).cast_mut().cast::<AnyObject>()
    }
}

impl<T: crate::runtime::Message> MessageReceiver for &crate::rc::Retained<T> {
    fn into_raw(self) -> *mut AnyObject {
        self.as_ptr().cast::<AnyObject>()
    }
}

impl<T: crate::runtime::Message> MessageReceiver for crate::rc::Allocated<T> {
    fn into_raw(self) -> *mut AnyObject {
        crate::rc::Allocated::into_raw(self).cast::<AnyObject>()
    }
}

impl<T: crate::runtime::Message> MessageReceiver for crate::rc::PartialInit<T> {
    fn into_raw(self) -> *mut AnyObject {
        crate::rc::PartialInit::into_raw(self).cast::<AnyObject>()
    }
}

/// Converts a partially initialized object into the receiver and class for super dispatch.
#[doc(hidden)]
pub trait SuperReceiver {
    fn into_super(self) -> (*mut AnyObject, *const crate::runtime::AnyClass);
}

impl<T: crate::runtime::DefinedClass> SuperReceiver for crate::rc::PartialInit<T> {
    fn into_super(self) -> (*mut AnyObject, *const crate::runtime::AnyClass) {
        (
            crate::rc::PartialInit::into_raw(self).cast::<AnyObject>(),
            T::__superclass(),
        )
    }
}

impl<T: crate::runtime::ClassType> SuperReceiver for &T {
    fn into_super(self) -> (*mut AnyObject, *const crate::runtime::AnyClass) {
        (
            (self as *const T).cast_mut().cast::<AnyObject>(),
            T::__superclass(),
        )
    }
}

#[doc(hidden)]
/// Adjust an owned Rust result immediately before returning through the Objective-C ABI.
///
/// # Safety
///
/// The result must be returned directly to Objective-C, without running its Rust destructor.
pub unsafe fn prepare_method_return<R: crate::Encode>(value: R, selector: &str) -> R {
    if R::IS_OBJECT_OWNERSHIP && !selector_returns_retained(selector) {
        // SAFETY: Ownership wrappers declare the object pointer ABI through Encode.
        // Objective-C receives the pointer without running the wrapper's destructor.
        let pointer = unsafe { std::ptr::read((&raw const value).cast::<*mut AnyObject>()) };
        // SAFETY: The wrapper transfers its +1 ownership to the autorelease pool.
        unsafe { crate::ffi::objc_autorelease(pointer) };
    }
    value
}

#[doc(hidden)]
pub fn selector_returns_retained(selector: &str) -> bool {
    fn is_family(selector: &str, family: &str) -> bool {
        selector.starts_with(family)
            && selector[family.len()..]
                .chars()
                .next()
                .is_none_or(|character| !character.is_ascii_lowercase())
    }

    let selector = selector.trim_start_matches('_');
    selector == "retain"
        || ["alloc", "init", "new", "copy", "mutableCopy"]
            .iter()
            .any(|family| is_family(selector, family))
}
message_send_impl!();
message_send_impl!(a: A);
message_send_impl!(a: A, b: B);
message_send_impl!(a: A, b: B, c: C);
message_send_impl!(a: A, b: B, c: C, d: D);
message_send_impl!(a: A, b: B, c: C, d: D, e: E);
message_send_impl!(a: A, b: B, c: C, d: D, e: E, f: F);
message_send_impl!(a: A, b: B, c: C, d: D, e: E, f: F, g: G);
message_send_impl!(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H);
message_send_impl!(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I);
message_send_impl!(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J);
message_send_impl!(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J, k: K);
message_send_impl!(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J, k: K, l: L);

/// Send message to object
#[macro_export]
macro_rules! msg_send {
    (super($receiver:expr), $sel:ident) => ({
        let (__receiver, __superclass) = $crate::macros::SuperReceiver::into_super($receiver);
        $crate::macros::MessageSend::invoke_super(
            __receiver,
            __superclass,
            $crate::sel!($sel),
            (),
            $crate::macros::selector_returns_retained(stringify!($sel)),
        )
    });
    (super($receiver:expr) $(,$sel:ident : $arg:expr)+) => ({
        let (__receiver, __superclass) = $crate::macros::SuperReceiver::into_super($receiver);
        $crate::macros::MessageSend::invoke_super(
            __receiver,
            __superclass,
            $crate::sel!($($sel:)+),
            ($($arg,)+),
            $crate::macros::selector_returns_retained(stringify!($($sel)*)),
        )
    });
    ($receiver:expr, $sel:ident) => (
        $crate::macros::MessageSend::invoke(
            $crate::macros::MessageReceiver::into_raw($receiver),
            $crate::sel!($sel),
            (),
            $crate::macros::selector_returns_retained(stringify!($sel)),
        )
    );
    ($receiver:expr $(,$sel:ident : $arg:expr)+) => (
        $crate::macros::MessageSend::invoke(
            $crate::macros::MessageReceiver::into_raw($receiver),
            $crate::sel!($($sel:)+),
            ($($arg,)+),
            $crate::macros::selector_returns_retained(stringify!($($sel)*)),
        )
    );
}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::ffi::{CStr, c_void};

    use crate::ffi::sel_getName;
    use crate::runtime::AnyObject;

    #[link(name = "Foundation", kind = "framework")]
    unsafe extern "C" {}

    #[test]
    fn test_message_send_no_args() {
        // SAFETY: NSString is a valid Foundation class; alloc returns a valid uninitialized object.
        let ns: *mut AnyObject = unsafe { msg_send![class!(NSString), alloc] };
        assert!(!ns.is_null());
        // SAFETY: `ns` is a valid uninitialized NSString from alloc; init always succeeds.
        let ns: *mut AnyObject = unsafe { msg_send![ns, init] };
        assert!(!ns.is_null());
        // SAFETY: `ns` is a fully initialized NSString; length is a valid method returning NSUInteger.
        let length: u64 = unsafe { msg_send![ns, length] };
        assert_eq!(length, 0);
        // SAFETY: `ns` is a valid ObjC object; release decrements the retain count.
        unsafe {
            let _: () = msg_send![ns, release];
        }
    }

    #[test]
    fn test_message_send_alloc_init() {
        // SAFETY: NSObject is a valid Foundation class; alloc/init/release are standard ObjC methods.
        unsafe {
            let obj: *mut AnyObject = msg_send![class!(NSObject), alloc];
            assert!(!obj.is_null());
            let obj: *mut AnyObject = msg_send![obj, init];
            assert!(!obj.is_null());
            let _: () = msg_send![obj, release];
        }
    }

    #[test]
    fn test_message_send_two_args() {
        // SAFETY: all classes are valid Foundation types; all selectors match their declared signatures.
        unsafe {
            let dict: *mut AnyObject = msg_send![class!(NSMutableDictionary), new];
            assert!(!dict.is_null());
            let key: *mut AnyObject = msg_send![class!(NSString), new];
            let val: *mut AnyObject = msg_send![class!(NSObject), new];
            let _: () = msg_send![dict, setObject: val, forKey: key];
            let count: u64 = msg_send![dict, count];
            assert_eq!(count, 1);
            let _: () = msg_send![key, release];
            let _: () = msg_send![val, release];
            let _: () = msg_send![dict, release];
        }
    }

    #[test]
    fn test_message_send_three_args() {
        // SAFETY: all classes are valid Foundation types; all selectors match their declared signatures.
        unsafe {
            let ns: *mut AnyObject = msg_send![class!(NSString), alloc];
            let ns: *mut AnyObject = msg_send![ns,
                initWithBytes: b"hello".as_ptr() as *const c_void,
                length: 5u64,
                encoding: 4u64
            ];
            assert!(!ns.is_null());
            let len: u64 = msg_send![ns, length];
            assert_eq!(len, 5);
            let _: () = msg_send![ns, release];
        }
    }

    #[repr(C)]
    struct NSRange {
        location: u64,
        length: u64,
    }
    // SAFETY: NSRange is typedef'd from `struct _NSRange { u64 location; u64 length; }`;
    // the ObjC runtime uses the underlying struct name `_NSRange` in type encodings.
    unsafe impl crate::Encode for NSRange {
        const ENCODING: crate::Encoding = crate::Encoding::Struct(
            "_NSRange",
            &[crate::Encoding::ULongLong, crate::Encoding::ULongLong],
        );
    }

    unsafe fn make_nsstring(bytes: &[u8]) -> *mut AnyObject {
        // SAFETY: NSString is a valid Foundation class; initWithBytes:length:encoding: is a standard initializer.
        unsafe {
            let ns: *mut AnyObject = msg_send![class!(NSString), alloc];
            msg_send![ns,
                initWithBytes: bytes.as_ptr() as *const c_void,
                length: bytes.len() as u64,
                encoding: 4u64
            ]
        }
    }

    #[test]
    fn test_message_send_four_args() {
        // SAFETY: all classes are valid Foundation types; all selectors match their declared signatures.
        unsafe {
            let src = make_nsstring(b"hello world");
            let from = make_nsstring(b"world");
            let to = make_nsstring(b"rust");
            let result: *mut AnyObject = msg_send![src,
                stringByReplacingOccurrencesOfString: from,
                withString: to,
                options: 0u64,
                range: NSRange { location: 0, length: 11 }
            ];
            assert!(!result.is_null());
            let len: u64 = msg_send![result, length];
            assert_eq!(len, 10); // "hello rust" = 10 chars
            let _: () = msg_send![src, release];
            let _: () = msg_send![from, release];
            let _: () = msg_send![to, release];
        }
    }

    #[test]
    fn test_sel_macro_single_name() {
        // SAFETY: `sel!(length)` is a registered selector; sel_getName returns a valid null-terminated C string.
        let name = unsafe { CStr::from_ptr(sel_getName(sel!(length).0)) };
        assert_eq!(name.to_bytes(), b"length");
    }

    #[test]
    fn test_sel_macro_multi_name() {
        // SAFETY: `sel!(setObject: forKey:)` is a registered selector; sel_getName returns a valid null-terminated C string.
        let name = unsafe { CStr::from_ptr(sel_getName(sel!(setObject: forKey:).0)) };
        assert_eq!(name.to_bytes(), b"setObject:forKey:");
    }

    #[test]
    fn test_class_macro_returns_null_for_unknown() {
        let cls = class!(NoSuchClassXyzAbc999);
        assert!(cls.is_null(), "unknown class name must return null");
    }

    #[test]
    fn test_class_macro_returns_non_null_for_known() {
        let cls = class!(NSObject);
        assert!(!cls.is_null(), "NSObject must always be resolvable");
    }

    #[test]
    fn test_msg_send_retain_count() {
        // SAFETY: NSObject is a valid Foundation class; alloc/init/retain/release/retainCount are standard.
        unsafe {
            let obj: *mut AnyObject = msg_send![class!(NSObject), alloc];
            let obj: *mut AnyObject = msg_send![obj, init];
            let rc1: u64 = msg_send![obj, retainCount];
            let _: *mut AnyObject = msg_send![obj, retain];
            let rc2: u64 = msg_send![obj, retainCount];
            assert_eq!(rc2, rc1 + 1);
            let _: () = msg_send![obj, release];
            let _: () = msg_send![obj, release];
        }
    }

    #[test]
    fn test_msg_send_bool_return() {
        // SAFETY: NSString is a valid Foundation class; isEqualToString: is a standard selector.
        unsafe {
            use crate::runtime::Bool;
            let ns: *mut AnyObject = msg_send![class!(NSString), alloc];
            let ns: *mut AnyObject = msg_send![ns, init];
            // An empty NSString is equal to another empty NSString.
            let other: *mut AnyObject = msg_send![class!(NSString), new];
            let equal: Bool = msg_send![ns, isEqualToString: other];
            assert_eq!(equal, Bool::YES);
            let _: () = msg_send![ns, release];
            let _: () = msg_send![other, release];
        }
    }
}
