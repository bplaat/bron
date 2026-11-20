/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;
use std::ptr::NonNull;

use libogc_sys::{
    DCInvalidateRange, GXRModeObj, SYS_AllocateFramebuffer, SYS_BASE_CACHED, SYS_BASE_UNCACHED,
    free, memalign,
};

struct HeapBuffer(NonNull<c_void>);

impl HeapBuffer {
    fn from_raw(pointer: *mut c_void) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

pub struct Framebuffer(HeapBuffer);

impl Framebuffer {
    pub fn new(mode: &GXRModeObj) -> Option<Self> {
        HeapBuffer::from_raw(unsafe { SYS_AllocateFramebuffer(mode) }).map(Self)
    }

    pub fn scanout(&self) -> *mut c_void {
        (self.0.as_ptr() as u32).wrapping_add(SYS_BASE_UNCACHED.wrapping_sub(SYS_BASE_CACHED))
            as *mut c_void
    }
}

pub struct AlignedBuffer {
    buffer: HeapBuffer,
    size: u32,
}

impl AlignedBuffer {
    pub fn new(size: usize) -> Option<Self> {
        if size == 0 || !size.is_multiple_of(32) || size > u32::MAX as usize {
            return None;
        }
        let buffer = HeapBuffer::from_raw(unsafe { memalign(32, size) })?;
        Some(Self {
            buffer,
            size: size as u32,
        })
    }

    pub fn as_ptr(&self) -> *mut c_void {
        self.buffer.as_ptr()
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn zero(&mut self) {
        unsafe { std::ptr::write_bytes(self.as_ptr(), 0, self.size as usize) }
    }

    pub fn invalidate(&mut self) {
        unsafe { DCInvalidateRange(self.as_ptr(), self.size) }
    }
}

impl Drop for HeapBuffer {
    fn drop(&mut self) {
        unsafe { free(self.0.as_ptr()) }
    }
}
