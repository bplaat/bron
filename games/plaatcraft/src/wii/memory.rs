/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;
use std::ptr::NonNull;

use libogc_sys::{
    DCFlushRange, GXRModeObj, SYS_AllocateFramebuffer, SYS_BASE_CACHED, SYS_BASE_UNCACHED, free,
    memalign,
};

struct HeapBuffer(NonNull<c_void>);

impl HeapBuffer {
    fn from_raw(pointer: *mut c_void) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }

    fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for HeapBuffer {
    fn drop(&mut self) {
        unsafe { free(self.0.as_ptr()) }
    }
}

pub(crate) struct Framebuffer(HeapBuffer);

impl Framebuffer {
    pub(crate) fn new(mode: &GXRModeObj) -> Option<Self> {
        HeapBuffer::from_raw(unsafe { SYS_AllocateFramebuffer(mode) }).map(Self)
    }

    pub(crate) fn scanout(&self) -> *mut c_void {
        (self.0.as_ptr() as u32).wrapping_add(SYS_BASE_UNCACHED.wrapping_sub(SYS_BASE_CACHED))
            as *mut c_void
    }
}

pub(crate) struct AlignedBuffer {
    buffer: HeapBuffer,
    size: u32,
}

impl AlignedBuffer {
    pub(crate) fn new_zeroed(size: usize) -> Option<Self> {
        if size == 0 || !size.is_multiple_of(32) || size > u32::MAX as usize {
            return None;
        }
        let buffer = HeapBuffer::from_raw(unsafe { memalign(32, size) })?;
        unsafe { std::ptr::write_bytes(buffer.as_ptr(), 0, size) };
        Some(Self {
            buffer,
            size: size as u32,
        })
    }

    pub(crate) fn as_ptr(&self) -> *mut c_void {
        self.buffer.as_ptr()
    }

    pub(crate) fn size(&self) -> u32 {
        self.size
    }
}

/// Writes the CPU cache back to memory, so the GPU reads what the CPU wrote.
pub(crate) fn flush<T>(data: &[T]) {
    unsafe { DCFlushRange(data.as_ptr().cast_mut().cast(), size_of_val(data) as u32) }
}

// Every allocation is 32-byte aligned, so byte vectors can be GX display lists and arrays
pub(crate) struct CHeapAllocator;

unsafe impl GlobalAlloc for CHeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { memalign(layout.align().max(32), layout.size().max(1)).cast() }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, _layout: Layout) {
        unsafe { free(pointer.cast()) }
    }
}

#[global_allocator]
static ALLOCATOR: CHeapAllocator = CHeapAllocator;
