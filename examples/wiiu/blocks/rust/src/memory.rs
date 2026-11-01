/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::alloc::{GlobalAlloc, Layout};

use wut_sys::{free, memalign};

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
