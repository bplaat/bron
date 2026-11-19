/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::c_void;

use wut_sys::DCFlushRange;

/// Writes the CPU cache back to memory, so the GPU and the DSP read what the CPU wrote.
pub(crate) fn flush<T>(data: &[T]) {
    unsafe {
        DCFlushRange(
            data.as_ptr().cast_mut().cast::<c_void>(),
            size_of_val(data) as u32,
        )
    }
}
