/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Integration tests for `define_class!` and `extern_class!`.
//!
//! These live in an integration test file rather than inline `#[cfg(test)]` because both macros
//! generate `::objc2::` absolute paths in their output, which only resolve when the caller is an
//! external crate -- as integration tests are.

#![cfg(target_vendor = "apple")]
#![allow(unsafe_code, clippy::undocumented_unsafe_blocks)]

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::ffi::{objc_msgSendSuper, objc_super};
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{class, define_class, extern_class, msg_send, sel};

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

// MARK: define_class! - no methods

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TestNoMethodsClass"]
    struct NoMethodsClass;
);

#[test]
fn test_define_class_no_methods_class_ptr_non_null() {
    assert!(!NoMethodsClass::class().is_null());
}

#[test]
fn test_define_class_class_idempotent() {
    assert_eq!(NoMethodsClass::class(), NoMethodsClass::class());
}

#[test]
fn test_define_class_instantiation() {
    let obj: *mut AnyObject = unsafe { msg_send![NoMethodsClass::class(), new] };
    assert!(!obj.is_null());
    unsafe {
        let _: () = msg_send![obj, release];
    }
}

// MARK: define_class! - with methods

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TestMethodClass"]
    struct MethodClass;

    impl MethodClass {
        #[unsafe(method(answer))]
        const fn _answer(&self) -> i64 {
            42
        }

        #[unsafe(method(double:))]
        const fn _double(&self, n: i64) -> i64 {
            n * 2
        }

        #[unsafe(method(add:to:))]
        const fn _add(&self, a: i64, b: i64) -> i64 {
            a + b
        }
    }
);

#[test]
fn test_define_class_zero_arg_method_return() {
    let obj: *mut AnyObject = unsafe { msg_send![MethodClass::class(), new] };
    let result: i64 = unsafe { msg_send![obj, answer] };
    assert_eq!(result, 42);
    unsafe {
        let _: () = msg_send![obj, release];
    }
}

#[test]
fn test_define_class_one_arg_method_return() {
    let obj: *mut AnyObject = unsafe { msg_send![MethodClass::class(), new] };
    let result: i64 = unsafe { msg_send![obj, double: 7i64] };
    assert_eq!(result, 14);
    unsafe {
        let _: () = msg_send![obj, release];
    }
}

#[test]
fn test_define_class_two_arg_method_return() {
    let obj: *mut AnyObject = unsafe { msg_send![MethodClass::class(), new] };
    let result: i64 = unsafe { msg_send![obj, add: 3i64, to: 4i64] };
    assert_eq!(result, 7);
    unsafe {
        let _: () = msg_send![obj, release];
    }
}

// MARK: define_class! - with ivars

struct CounterIvars {
    count: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TestCounterClass"]
    #[ivars = CounterIvars]
    struct CounterClass;

    impl CounterClass {
        #[unsafe(method_id(init))]
        fn _init(this: Allocated<Self>) -> Option<Retained<Self>> {
            unsafe { msg_send![super(this.set_ivars(CounterIvars { count: Cell::new(0) })), init] }
        }

        #[unsafe(method(increment))]
        fn _increment(&self) {
            let c = self.ivars().count.get();
            self.ivars().count.set(c + 1);
        }

        #[unsafe(method(count))]
        fn _count(&self) -> i64 {
            self.ivars().count.get()
        }
    }
);

#[test]
fn test_define_class_ivars_explicitly_initialized() {
    let obj: *mut AnyObject = unsafe { msg_send![CounterClass::class(), alloc] };
    let obj: *mut AnyObject = unsafe { msg_send![obj, init] };
    let count: i64 = unsafe { msg_send![obj, count] };
    assert_eq!(count, 0);
    unsafe {
        let _: () = msg_send![obj, release];
    }
}

static IVARS_DROPPED: AtomicBool = AtomicBool::new(false);
static FAILED_IVARS_DROPPED: AtomicBool = AtomicBool::new(false);

struct DropMarker;

impl Drop for DropMarker {
    fn drop(&mut self) {
        IVARS_DROPPED.store(true, Ordering::SeqCst);
    }
}

struct DropIvars {
    _marker: DropMarker,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TestDropIvarsClass"]
    #[ivars = DropIvars]
    struct DropIvarsClass;

    impl DropIvarsClass {
        #[unsafe(method_id(init))]
        fn _init(this: Allocated<Self>) -> Option<Retained<Self>> {
            unsafe {
                msg_send![
                    super(this.set_ivars(DropIvars {
                        _marker: DropMarker,
                    })),
                    init
                ]
            }
        }
    }
);

#[test]
fn test_define_class_drops_initialized_ivars() {
    IVARS_DROPPED.store(false, Ordering::SeqCst);
    let object: Retained<DropIvarsClass> = unsafe { msg_send![DropIvarsClass::class(), new] };
    assert!(!IVARS_DROPPED.load(Ordering::SeqCst));
    drop(object);
    assert!(IVARS_DROPPED.load(Ordering::SeqCst));
}

struct FailedDropMarker;

impl Drop for FailedDropMarker {
    fn drop(&mut self) {
        FAILED_IVARS_DROPPED.store(true, Ordering::SeqCst);
    }
}

struct FailedIvars {
    _marker: FailedDropMarker,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TestFailedIvarsClass"]
    #[ivars = FailedIvars]
    struct FailedIvarsClass;

    impl FailedIvarsClass {
        #[unsafe(method_id(init))]
        fn _init(this: Allocated<Self>) -> Option<Retained<Self>> {
            drop(this.set_ivars(FailedIvars {
                _marker: FailedDropMarker,
            }));
            None
        }
    }
);

#[test]
fn test_define_class_drops_ivars_when_initialization_fails() {
    FAILED_IVARS_DROPPED.store(false, Ordering::SeqCst);
    let object: Option<Retained<FailedIvarsClass>> =
        unsafe { msg_send![FailedIvarsClass::class(), new] };
    assert!(object.is_none());
    assert!(FAILED_IVARS_DROPPED.load(Ordering::SeqCst));
}

struct ValueIvars {
    value: i64,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TestValueIvarsClass"]
    #[ivars = ValueIvars]
    struct ValueIvarsClass;

    impl ValueIvarsClass {
        #[unsafe(method_id(initWithValue:))]
        fn _init_with_value(
            this: Allocated<Self>,
            value: i64,
        ) -> Option<Retained<Self>> {
            unsafe { msg_send![super(this.set_ivars(ValueIvars { value })), init] }
        }

        #[unsafe(method(value))]
        fn _value(&self) -> i64 {
            self.ivars().value
        }
    }
);

#[test]
fn test_define_class_initializer_argument_is_not_an_objective_c_argument() {
    let object: Allocated<ValueIvarsClass> = unsafe { msg_send![ValueIvarsClass::class(), alloc] };
    let object: Retained<ValueIvarsClass> = unsafe { msg_send![object, initWithValue: 42_i64] };
    let value: i64 = unsafe { msg_send![&object, value] };
    assert_eq!(value, 42);
}

#[test]
fn test_define_class_ivars_mutated_via_method() {
    let obj: *mut AnyObject = unsafe { msg_send![CounterClass::class(), alloc] };
    let obj: *mut AnyObject = unsafe { msg_send![obj, init] };
    unsafe {
        let _: () = msg_send![obj, increment];
    }
    unsafe {
        let _: () = msg_send![obj, increment];
    }
    unsafe {
        let _: () = msg_send![obj, increment];
    }
    let count: i64 = unsafe { msg_send![obj, count] };
    assert_eq!(count, 3);
    unsafe {
        let _: () = msg_send![obj, release];
    }
}

#[test]
fn test_define_class_ivars_independent_per_instance() {
    let a: *mut AnyObject = unsafe { msg_send![CounterClass::class(), alloc] };
    let a: *mut AnyObject = unsafe { msg_send![a, init] };
    let b: *mut AnyObject = unsafe { msg_send![CounterClass::class(), alloc] };
    let b: *mut AnyObject = unsafe { msg_send![b, init] };

    unsafe {
        let _: () = msg_send![a, increment];
    }
    unsafe {
        let _: () = msg_send![a, increment];
    }
    unsafe {
        let _: () = msg_send![b, increment];
    }

    let count_a: i64 = unsafe { msg_send![a, count] };
    let count_b: i64 = unsafe { msg_send![b, count] };
    assert_eq!(count_a, 2);
    assert_eq!(count_b, 1);

    unsafe {
        let _: () = msg_send![a, release];
    }
    unsafe {
        let _: () = msg_send![b, release];
    }
}

// MARK: define_class! - dealloc

static DEALLOCATED: AtomicBool = AtomicBool::new(false);

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TestDeallocClass"]
    struct DeallocClass;

    impl DeallocClass {
        #[unsafe(method(dealloc))]
        fn _dealloc(this: *mut Self) {
            DEALLOCATED.store(true, Ordering::SeqCst);
            // SAFETY: this is the object currently being deallocated, and NSObject is its super.
            unsafe {
                let super_info = objc_super {
                    receiver: this.cast::<AnyObject>(),
                    super_class: class!(NSObject).cast::<AnyClass>(),
                };
                let send: unsafe extern "C-unwind" fn(*const objc_super, *const c_void) =
                    std::mem::transmute(objc_msgSendSuper as *const c_void);
                send(&super_info, sel!(dealloc).0);
            }
        }
    }
);

#[test]
fn test_define_class_dealloc_uses_raw_receiver() {
    DEALLOCATED.store(false, Ordering::SeqCst);
    // SAFETY: DeallocClass inherits NSObject's new and release methods.
    unsafe {
        let object: *mut AnyObject = msg_send![DeallocClass::class(), new];
        let _: () = msg_send![object, release];
    }
    assert!(DEALLOCATED.load(Ordering::SeqCst));
}

// MARK: define_class! - return ownership

type ReturnOwnershipIvars = Retained<AnyObject>;

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = ReturnOwnershipIvars]
    struct ReturnOwnershipClass;

    impl ReturnOwnershipClass {
        #[unsafe(method_id(initWithObject:))]
        fn _init(this: Allocated<Self>, object: *mut AnyObject) -> Option<Retained<Self>> {
            unsafe { msg_send![super(this.set_ivars(Retained::retain(object).expect("non-null test object"))), init] }
        }

        #[unsafe(method(object))]
        fn _object(&self) -> Retained<AnyObject> {
            self.ivars().clone()
        }

        #[unsafe(method_id(optionalObject:))]
        fn _optional_object(&self, present: bool) -> Option<Retained<AnyObject>> {
            present.then(|| self.ivars().clone())
        }

        #[unsafe(method(rawObject))]
        fn _raw_object(&self) -> *mut AnyObject {
            self.ivars().as_ptr()
        }

        #[unsafe(method_id(newObject))]
        fn _new_object(&self) -> Retained<AnyObject> {
            self.ivars().clone()
        }

        #[unsafe(method_id(_copyObject))]
        fn _copy_object(&self) -> Option<Retained<AnyObject>> {
            Some(self.ivars().clone())
        }

        #[unsafe(method(newspaper))]
        fn _newspaper(&self) -> Retained<AnyObject> {
            self.ivars().clone()
        }
    }
);

#[test]
fn test_define_class_return_ownership() {
    unsafe {
        let object: Retained<AnyObject> = msg_send![class!(NSObject), new];
        let factory: Allocated<ReturnOwnershipClass> =
            msg_send![ReturnOwnershipClass::class(), alloc];
        let factory: Retained<ReturnOwnershipClass> =
            msg_send![factory, initWithObject: object.as_ptr()];
        let retain_count = || -> usize { msg_send![&object, retainCount] };
        let baseline = retain_count();

        // The Rust caller must keep one retain after the pool drains, then release it on drop.
        macro_rules! check_return {
            ($($selector:tt)*) => {
                let returned: Retained<AnyObject> = autoreleasepool(|_| {
                    msg_send![&factory, $($selector)*]
                });
                assert_eq!(returned.as_ptr(), object.as_ptr());
                assert_eq!(retain_count(), baseline + 1);
                drop(returned);
                assert_eq!(retain_count(), baseline);
            };
        }
        check_return!(object);
        check_return!(optionalObject: true);
        check_return!(newObject);
        check_return!(_copyObject);
        check_return!(newspaper);

        autoreleasepool(|_| {
            let absent: Option<Retained<AnyObject>> = msg_send![&factory, optionalObject: false];
            assert!(absent.is_none());
            let borrowed: *mut AnyObject = msg_send![&factory, rawObject];
            assert_eq!(borrowed, object.as_ptr());
        });
        assert_eq!(retain_count(), baseline);

        let direct = autoreleasepool(|_| factory._object());
        assert_eq!(retain_count(), baseline + 1);
        drop(direct);
        assert_eq!(retain_count(), baseline);
    }
}

// MARK: extern_class!

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "NSObject"]
    struct ExternNSObject;
);

extern_class!(
    #[unsafe(super(NSObject))]
    #[name = "NSString"]
    struct ExternNSString;
);

#[test]
fn test_extern_class_returns_non_null() {
    assert!(!ExternNSObject::class().is_null());
}

#[test]
fn test_extern_class_matches_class_macro() {
    assert_eq!(ExternNSObject::class(), class!(NSObject));
}

#[test]
fn test_extern_class_name_attr_selects_objc_class() {
    assert_eq!(ExternNSString::class(), class!(NSString));
}
