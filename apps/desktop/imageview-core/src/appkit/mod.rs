/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! AppKit support for ImageView and its Quick Look extensions.

use std::ffi::{CStr, CString, OsStr, c_char, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use bcanvas::{CanvasRenderingContext2d, NativeContext};
use objc2::msg_send;
use objc2::runtime::{AnyObject as Object, Bool};

use self::headers::*;
pub use self::headers::{
    __CFConstantStringClassReference, CFConstString, NS_VIEW_HEIGHT_SIZABLE, NS_VIEW_WIDTH_SIZABLE,
    Point, Rect, Size, ns_string,
};
pub use self::media_view::create_media_view;
// The macro of the same name, which is exported from the crate root.
pub use crate::ns_string;
use crate::{Media, decode_media};

mod headers;
mod media_view;

/// Runs `work` asynchronously on a user-initiated global GCD queue.
pub fn dispatch_async<F>(work: F)
where
    F: FnOnce() + Send + 'static,
{
    // SAFETY: The system user-initiated global queue exists for the process lifetime.
    let queue = unsafe { dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0) };
    dispatch_to(queue, work);
}

/// Runs `work` asynchronously on the main GCD queue.
pub fn dispatch_async_main<F>(work: F)
where
    F: FnOnce() + Send + 'static,
{
    // SAFETY: The exported system main queue exists for the process lifetime. Apple's
    // dispatch_get_main_queue is an inline C function, so bind its backing object directly.
    let queue = std::ptr::addr_of_mut!(DISPATCH_MAIN_QUEUE).cast::<c_void>();
    dispatch_to(queue, work);
}

fn dispatch_to<F>(queue: *mut c_void, work: F)
where
    F: FnOnce() + Send + 'static,
{
    extern "C" fn invoke<F>(context: *mut c_void)
    where
        F: FnOnce() + Send + 'static,
    {
        // SAFETY: dispatch_to created this allocation for this one GCD invocation.
        let work = unsafe { Box::from_raw(context.cast::<Option<F>>()) }
            .take()
            .expect("GCD work was already invoked");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            objc2::rc::autoreleasepool(|_| work());
        }));
        if result.is_err() {
            std::process::abort();
        }
    }

    let context = Box::into_raw(Box::new(Some(work))).cast();
    // SAFETY: The context stays allocated until GCD invokes the function exactly once.
    unsafe {
        dispatch_async_f(queue, context, invoke::<F>);
    }
}

/// Loads a supported image from a file URL.
///
/// # Safety
///
/// `url` must point to a valid `NSURL` for the duration of this call.
pub unsafe fn load_media(url: *mut Object) -> Result<Media, String> {
    // SAFETY: url is valid. Quick Look passes security-scoped URLs, and ordinary URLs simply
    // return false without changing their access state.
    let scoped: Bool = unsafe { msg_send![url, startAccessingSecurityScopedResource] };
    // SAFETY: The caller supplies a live file URL.
    let path = unsafe { file_path(url) };
    let result = path
        .ok_or_else(|| String::from("Could not read the image"))
        .and_then(|path| std::fs::read(path).map_err(|_| String::from("Could not read the image")))
        .and_then(|bytes| decode_media(&bytes));
    if scoped.as_bool() {
        // SAFETY: This balances the successful start call above.
        unsafe {
            let _: () = msg_send![url, stopAccessingSecurityScopedResource];
        }
    }
    result
}

// Returns the Rust path represented by a file URL.
//
// # Safety
//
// `url` must point to a valid file `NSURL` for this call.
unsafe fn file_path(url: *mut Object) -> Option<PathBuf> {
    // SAFETY: NSURL owns the representation, which is copied into the Rust path before return.
    let representation: *const c_char = unsafe { msg_send![url, fileSystemRepresentation] };
    if representation.is_null() {
        return None;
    }
    // SAFETY: NSURL returns a null-terminated file-system representation.
    let bytes = unsafe { CStr::from_ptr(representation) }.to_bytes();
    Some(PathBuf::from(OsStr::from_bytes(bytes)))
}

/// Draws into a Core Graphics context with a bottom left origin, like the one of a Quick Look
/// thumbnail, with the top left canvas coordinates of `size`.
///
/// # Safety
///
/// `context` must be a valid `CGContext` for the duration of this call.
pub unsafe fn draw_bottom_up(
    context: *mut c_void,
    size: Size,
    draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
) {
    // SAFETY: The caller supplies a live context, whose state is restored before returning.
    unsafe {
        CGContextSaveGState(context);
        CGContextTranslateCTM(context, 0.0, size.height);
        CGContextScaleCTM(context, 1.0, -1.0);
        bcanvas::draw_native(
            NativeContext::CoreGraphics(context),
            size.width as f32,
            size.height as f32,
            1.0,
            draw,
        );
        CGContextRestoreGState(context);
    }
}

/// Creates an autoreleased `NSError` that carries `description` as its localized description.
///
/// # Safety
///
/// `domain` must point to a valid `NSString`, which [`ns_string!`] guarantees.
pub unsafe fn make_error(domain: *mut Object, description: &str) -> *mut Object {
    // SAFETY: The Foundation convenience constructors return autoreleased objects, and the caller
    // guarantees the domain is a valid string.
    unsafe {
        let user_info: *mut Object = msg_send![objc2::class!(NSDictionary),
            dictionaryWithObject: ns_string(description),
            forKey: ns_string!("NSLocalizedDescription")
        ];
        msg_send![objc2::class!(NSError),
            errorWithDomain: domain,
            code: 1isize,
            userInfo: user_info
        ]
    }
}

/// Hands the process over to the app extension entry point, which does not return.
pub fn extension_main() -> ! {
    let arguments: Vec<CString> = std::env::args_os()
        .map(|argument| {
            CString::new(argument.as_os_str().as_bytes())
                .expect("process argument contains a null byte")
        })
        .collect();
    let argument_pointers: Vec<*const c_char> =
        arguments.iter().map(|argument| argument.as_ptr()).collect();
    // SAFETY: The argument pointers stay alive for the non-returning extension entry point.
    unsafe {
        NSExtensionMain(argument_pointers.len() as i32, argument_pointers.as_ptr());
    }
    unreachable!("NSExtensionMain does not return");
}
