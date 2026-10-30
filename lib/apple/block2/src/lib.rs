/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [block2](https://crates.io/crates/block2) crate

#![cfg(target_vendor = "apple")]
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::ptr::NonNull;

use objc2::encode::{Encode, Encoding};

#[link(name = "System", kind = "dylib")]
unsafe extern "C-unwind" {
    static _NSConcreteStackBlock: u8;
    fn _Block_copy(block: *const c_void) -> *mut c_void;
    fn _Block_release(block: *const c_void);
}

#[repr(C)]
struct BlockDescriptor {
    reserved: usize,
    size: usize,
    copy: unsafe extern "C-unwind" fn(*mut c_void, *const c_void),
    dispose: unsafe extern "C-unwind" fn(*mut c_void),
}

const BLOCK_HAS_COPY_DISPOSE: i32 = 1 << 25;

/// An Objective-C block. `F` is the function signature (e.g. `dyn Fn(i64)`).
/// This type is `repr(C)` matching the ObjC block ABI, so `&Block<F>` is a thin pointer.
#[repr(C)]
pub struct Block<F: ?Sized> {
    _isa: *const c_void,
    _flags: i32,
    _reserved: i32,
    _invoke: Option<unsafe extern "C-unwind" fn()>,
    _descriptor: *const BlockDescriptor,
    _marker: PhantomData<*const F>,
}

// A block argument is encoded as `@?` (an object which is a block)
// SAFETY: A block pointer always has encoding `@?` in the ObjC type system, and `F: BlockFn`
// bounds the parameters and return type to be encodable too.
unsafe impl<F: ?Sized + BlockFn> Encode for &Block<F> {
    const ENCODING: Encoding = Encoding::Block;
}

mod private {
    pub trait Sealed<A, R> {}
}

/// Types that represent closure parameters/arguments and return types in a block.
///
/// This is implemented for [`dyn`] [`Fn`] closures with up to 3 parameters, where each
/// parameter and the return type implement [`Encode`].
///
/// [`dyn`]: https://doc.rust-lang.org/std/keyword.dyn.html
///
/// # Safety
///
/// This is a sealed trait, and should not need to be implemented.
pub unsafe trait BlockFn: private::Sealed<Self::Args, Self::Output> {
    /// The parameters/arguments to the block.
    type Args;

    /// The return type of the block.
    type Output;

    /// Calls the given invoke function with the block and arguments.
    ///
    /// # Safety
    ///
    /// `invoke` must be the invoke function of `block` and `block` must be a live block.
    #[doc(hidden)]
    unsafe fn __call_block(
        invoke: unsafe extern "C-unwind" fn(),
        block: *mut Block<Self>,
        args: Self::Args,
    ) -> Self::Output;
}

/// Types that may be converted into a block.
///
/// This is implemented for [`Fn`] closures of up to 3 parameters, where each parameter and the
/// return type implement [`Encode`].
///
/// # Safety
///
/// This is a sealed trait, and should not need to be implemented.
pub unsafe trait IntoBlock<'f, A, R>: private::Sealed<A, R> {
    /// The type-erased `dyn Fn(...Args) -> R + 'f`.
    type Dyn: ?Sized + BlockFn<Args = A, Output = R>;

    #[doc(hidden)]
    fn __get_invoke_stack_block() -> unsafe extern "C-unwind" fn();
}

macro_rules! impl_traits {
    ($($a:ident: $t:ident),*) => {
        impl<$($t: Encode,)* R: Encode, Closure> private::Sealed<($($t,)*), R> for Closure
        where
            Closure: ?Sized + Fn($($t),*) -> R,
        {}

        // SAFETY: The invoke function is transmuted back to the exact signature it was created
        // with in `IntoBlock::__get_invoke_stack_block` or by the Objective-C compiler.
        unsafe impl<$($t: Encode,)* R: Encode> BlockFn for dyn Fn($($t),*) -> R + '_ {
            type Args = ($($t,)*);
            type Output = R;

            unsafe fn __call_block(
                invoke: unsafe extern "C-unwind" fn(),
                block: *mut Block<Self>,
                ($($a,)*): Self::Args,
            ) -> Self::Output {
                // SAFETY: The block ABI stores invoke pointers type-erased, the caller guarantees
                // that `invoke` belongs to `block` and has exactly this signature.
                unsafe {
                    let invoke: unsafe extern "C-unwind" fn(*mut Block<Self> $(, $t)*) -> R =
                        std::mem::transmute(invoke);
                    invoke(block $(, $a)*)
                }
            }
        }

        // SAFETY: `Dyn` matches the closure signature and the invoke function below.
        unsafe impl<'f, $($t: Encode,)* R: Encode, Closure> IntoBlock<'f, ($($t,)*), R> for Closure
        where
            Closure: Fn($($t),*) -> R + 'f,
        {
            type Dyn = dyn Fn($($t),*) -> R + 'f;

            fn __get_invoke_stack_block() -> unsafe extern "C-unwind" fn() {
                unsafe extern "C-unwind" fn invoke<$($t,)* R, Closure: Fn($($t),*) -> R>(
                    block: *mut RcBlockInner<Closure>,
                    $($a: $t,)*
                ) -> R {
                    // SAFETY: `block` points to a live `RcBlockInner<Closure>` owned by the
                    // invoking block; it stays alive for the duration of this call.
                    let closure = unsafe { &*std::ptr::addr_of!((*block).closure) };
                    closure($($a),*)
                }

                // SAFETY: Only the pointer type is erased, `BlockFn::__call_block` transmutes it
                // back to the same signature before calling it.
                unsafe {
                    std::mem::transmute::<
                        unsafe extern "C-unwind" fn(*mut RcBlockInner<Closure> $(, $t)*) -> R,
                        unsafe extern "C-unwind" fn(),
                    >(invoke)
                }
            }
        }
    };
}
impl_traits!();
impl_traits!(a: A);
impl_traits!(a: A, b: B);
impl_traits!(a: A, b: B, c: C);

// Inner heap layout for `RcBlock`: ObjC block header immediately followed by the closure.
#[repr(C)]
struct RcBlockInner<Closure> {
    block: Block<Closure>,
    closure: Closure,
}

struct BlockDescriptorFor<Closure>(PhantomData<Closure>);

impl<Closure> BlockDescriptorFor<Closure> {
    const VALUE: BlockDescriptor = BlockDescriptor {
        reserved: 0,
        size: size_of::<RcBlockInner<Closure>>(),
        copy: copy_closure::<Closure>,
        dispose: dispose_closure::<Closure>,
    };
}

const unsafe extern "C-unwind" fn copy_closure<Closure>(_: *mut c_void, _: *const c_void) {
    // The initial stack block is copied exactly once. The runtime has already moved the closure
    // bytes into the heap block, and RcBlock::new forgets the stack copy.
}

unsafe extern "C-unwind" fn dispose_closure<Closure>(block: *mut c_void) {
    // SAFETY: The Blocks runtime calls this exactly once when the heap block's final reference is
    // released. RcBlock::new transferred ownership of the closure into that block.
    unsafe {
        std::ptr::drop_in_place(std::ptr::addr_of_mut!(
            (*block.cast::<RcBlockInner<Closure>>()).closure
        ));
    }
}

/// A heap-allocated ObjC block wrapping a Rust closure.
pub struct RcBlock<F: ?Sized> {
    inner: NonNull<Block<F>>,
}

impl<F: ?Sized> RcBlock<F> {
    /// Construct a `RcBlock` with the given closure.
    ///
    /// The closure will be copied to the heap on construction. When the block is called, it will
    /// return the value that results from calling the closure.
    pub fn new<'f, A, R, Closure>(closure: Closure) -> Self
    where
        Closure: IntoBlock<'f, A, R, Dyn = F>,
    {
        let stack = RcBlockInner {
            block: Block {
                // SAFETY: `_NSConcreteStackBlock` is a valid extern static exported by
                // libSystem; it is always initialised before any Rust code runs.
                _isa: std::ptr::addr_of!(_NSConcreteStackBlock).cast(),
                _flags: BLOCK_HAS_COPY_DISPOSE,
                _reserved: 0,
                _invoke: Some(Closure::__get_invoke_stack_block()),
                _descriptor: &BlockDescriptorFor::<Closure>::VALUE,
                _marker: PhantomData,
            },
            closure,
        };
        let mut stack = ManuallyDrop::new(stack);
        // SAFETY: `stack` has the Objective-C stack block layout. `_Block_copy` promotes it to
        // independently owned heap storage. The no-op copy helper transfers the closure bytes.
        let inner = unsafe { _Block_copy((&stack.block as *const Block<Closure>).cast()) };
        let Some(inner) = NonNull::new(inner.cast()) else {
            // SAFETY: No heap block was created, so ownership still belongs to the stack value.
            unsafe { ManuallyDrop::drop(&mut stack) };
            panic!("_Block_copy returned null");
        };
        Self { inner }
    }
}

impl<F: ?Sized> Block<F> {
    /// Copy this block to the heap or retain it if it is already heap allocated.
    pub fn copy(&self) -> RcBlock<F> {
        // SAFETY: `self` is a live Objective-C block. `_Block_copy` returns an owned heap block
        // with the same function signature.
        let inner = unsafe { _Block_copy((self as *const Self).cast()) };
        RcBlock {
            inner: NonNull::new(inner.cast()).expect("_Block_copy returned null"),
        }
    }

    /// Call this block with the given arguments as a tuple.
    pub fn call(&self, args: F::Args) -> F::Output
    where
        F: BlockFn,
    {
        let invoke = self._invoke.unwrap_or_else(|| unreachable!());
        // SAFETY: `invoke` belongs to this live block and `F: BlockFn` matches its signature.
        // The closure is an `Fn`, so calling it through a shared reference is fine.
        unsafe { F::__call_block(invoke, NonNull::from(self).as_ptr(), args) }
    }
}

impl<F: ?Sized> Clone for RcBlock<F> {
    fn clone(&self) -> Self {
        self.deref().copy()
    }
}

impl<F: ?Sized> Deref for RcBlock<F> {
    type Target = Block<F>;
    fn deref(&self) -> &Block<F> {
        // SAFETY: `self.inner` is the live block returned by `_Block_copy` in `make` and remains
        // valid until `Drop::drop`; we hold `&self` so it cannot be dropped here.
        unsafe { self.inner.as_ref() }
    }
}

impl<F: ?Sized> Drop for RcBlock<F> {
    fn drop(&mut self) {
        // SAFETY: `self.inner` owns the reference returned by `_Block_copy` in `make`.
        unsafe { _Block_release(self.inner.as_ptr().cast()) };
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    use super::*;

    #[test]
    fn test_block_call_1_arg() {
        static RESULT: AtomicI32 = AtomicI32::new(0);
        let block = RcBlock::new(|x: i32| {
            RESULT.store(x * 2, Ordering::SeqCst);
        });
        block.call((21,));
        assert_eq!(RESULT.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn test_block_call_via_ref() {
        // Simulate how bwebview passes &*block to ObjC and then Rust calls it
        static RESULT: AtomicI32 = AtomicI32::new(0);
        let block = RcBlock::new(|x: i32| {
            RESULT.store(x + 10, Ordering::SeqCst);
        });
        let block_ref: &Block<dyn Fn(i32)> = &block;
        block_ref.call((32,));
        assert_eq!(RESULT.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn test_block_capture() {
        static RESULT: AtomicI32 = AtomicI32::new(0);
        let multiplier = 7i32;
        let block = RcBlock::new(move |x: i32| {
            RESULT.store(x * multiplier, Ordering::SeqCst);
        });
        block.call((6,));
        assert_eq!(RESULT.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn test_block_drop_runs_closure_drop() {
        let dropped = Arc::new(AtomicBool::new(false));
        struct DropGuard(Arc<AtomicBool>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let guard = DropGuard(dropped.clone());
        let block = RcBlock::new(move |_: i32| {
            let _ = &guard;
        });
        assert!(!dropped.load(Ordering::SeqCst));
        drop(block);
        assert!(
            dropped.load(Ordering::SeqCst),
            "closure should be dropped with RcBlock"
        );
    }

    #[test]
    fn test_system_block_copy_keeps_capture_alive() {
        let dropped = Arc::new(AtomicBool::new(false));
        struct DropGuard(Arc<AtomicBool>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let guard = DropGuard(dropped.clone());
        let block = RcBlock::new(move |_: i32| {
            let _ = &guard;
        });
        // SAFETY: `block` is a valid Objective-C block and remains alive for this call.
        let copied = unsafe { _Block_copy((&*block as *const Block<_>).cast()) };
        drop(block);
        assert!(!dropped.load(Ordering::SeqCst));

        // SAFETY: `_Block_copy` returned a retained copy of the same block signature.
        let copied = unsafe { &*copied.cast::<Block<dyn Fn(i32)>>() };
        copied.call((0,));
        // SAFETY: This balances the single ownership reference returned by `_Block_copy`.
        unsafe { _Block_release((copied as *const Block<dyn Fn(i32)>).cast()) };
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn test_clone_keeps_capture_alive() {
        let dropped = Arc::new(AtomicBool::new(false));
        struct DropGuard(Arc<AtomicBool>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let guard = DropGuard(dropped.clone());
        let block = RcBlock::new(move |_: i32| {
            let _ = &guard;
        });
        let cloned = block.clone();
        drop(block);
        assert!(!dropped.load(Ordering::SeqCst));
        cloned.call((0,));
        drop(cloned);
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn test_block_is_objective_c_object() {
        let block = RcBlock::new(|_: i32| {});
        // SAFETY: A block is an Objective-C object and responds to the NSObject self selector.
        unsafe {
            let object = (&*block as *const Block<_>).cast::<AnyObject>().cast_mut();
            let same: *mut AnyObject = msg_send![object, self];
            assert_eq!(same, object);
        }
    }

    #[test]
    fn test_block_call_1_arg_ret() {
        let block = RcBlock::new(|x: i32| -> i32 { x * 2 });
        assert_eq!(block.call((21,)), 42);
    }

    #[test]
    fn test_block_call_0_and_2_args() {
        let block = RcBlock::new(|| 42i32);
        assert_eq!(block.call(()), 42);
        let block = RcBlock::new(|a: i32, b: i64| i64::from(a) + b);
        assert_eq!(block.call((40, 2)), 42);
    }

    #[test]
    fn test_block_encode() {
        assert_eq!(<&Block<dyn Fn(i32)>>::ENCODING.to_string(), "@?");
    }
}
