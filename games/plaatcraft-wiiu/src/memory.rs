/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;

use wut_sys::{DCFlushRange, free, memalign};

/// Allocates from the default heap of Cafe OS through the runtime's memalign
pub struct CHeapAllocator;

unsafe impl GlobalAlloc for CHeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { memalign(layout.align(), layout.size().max(1)).cast() }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
        unsafe { free(pointer.cast()) }
    }
}

#[global_allocator]
static ALLOCATOR: CHeapAllocator = CHeapAllocator;

/// Writes the CPU cache back to memory, so the GPU and the DSP read what the CPU wrote.
pub fn flush<T>(data: &[T]) {
    unsafe {
        DCFlushRange(
            data.as_ptr().cast_mut().cast::<c_void>(),
            size_of_val(data) as u32,
        )
    }
}
