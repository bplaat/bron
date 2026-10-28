/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Quick Look preview extension for the image formats ImageView displays.

#![allow(unsafe_code)]

use std::ptr::null_mut;

use block2::{Block, RcBlock};
use imageview_appkit::{
    NS_VIEW_HEIGHT_SIZABLE, NS_VIEW_WIDTH_SIZABLE, Point, Rect, Size, create_image_view,
    dispatch_async, dispatch_async_main, extension_main, load_media, make_error, ns_string,
    preferred_content_size,
};
use objc2::ffi::class_addProtocol;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject as Object, AnyProtocol};
use objc2::{class, define_class, msg_send};

mod headers;

define_class!(
    #[unsafe(super(NSViewController))]
    #[name = "ImageViewPreviewViewController"]
    struct PreviewViewController;

    impl PreviewViewController {
        #[unsafe(method(loadView))]
        fn _load_view(&self) {
            self.load_view();
        }

        #[unsafe(method(preparePreviewOfFileAtURL:completionHandler:))]
        fn _prepare_preview(
            &self,
            url: *mut Object,
            completion: &Block<dyn Fn(*mut Object)>,
        ) {
            self.prepare_preview(url, completion);
        }
    }
);

struct PreviewCompletion(RcBlock<dyn Fn(*mut Object)>);

struct MainQueueObject(Retained<Object>);

// SAFETY: This wrapper only transports an AppKit object without accessing it. The pointer is only
// exposed after the value reaches a main-queue continuation.
unsafe impl Send for MainQueueObject {}

impl MainQueueObject {
    const fn as_ptr_on_main(&self) -> *mut Object {
        self.0.as_ptr()
    }
}

struct SendableUrl(Retained<Object>);

// SAFETY: NSURL is immutable and safe to read while this uniquely owned wrapper is on a worker.
unsafe impl Send for SendableUrl {}

impl SendableUrl {
    const fn as_ptr(&self) -> *mut Object {
        self.0.as_ptr()
    }
}

// SAFETY: Quick Look completion blocks are escaping blocks intended for asynchronous use. This
// wrapper moves its owned copy to the main queue and invokes it there exactly once.
unsafe impl Send for PreviewCompletion {}

impl PreviewCompletion {
    fn call(&self, error: *mut Object) {
        self.0.call((error,));
    }
}

impl PreviewViewController {
    fn load_view(&self) {
        // SAFETY: The controller owns the placeholder view after setView:.
        unsafe {
            let view: Allocated<Object> = msg_send![class!(NSView), alloc];
            let view: Retained<Object> = msg_send![view,
                initWithFrame: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size { width: 640.0, height: 480.0 },
                }
            ];
            let this = self as *const Self as *mut Object;
            let _: () = msg_send![this, setView: view.as_ptr()];
        }
    }

    fn prepare_preview(&self, url: *mut Object, completion: &Block<dyn Fn(*mut Object)>) {
        // SAFETY: Quick Look supplied live objects. Copies keep them alive until the asynchronous
        // load and main-queue UI continuation have finished.
        let (this, url, completion) = unsafe {
            let this = self as *const Self as *mut Object;
            // Quick Look may reuse a controller for another file. Clear the previous preview now
            // so a failed replacement cannot leave stale media visible.
            let root: *mut Object = msg_send![this, view];
            let empty: *mut Object = msg_send![class!(NSArray), array];
            let _: () = msg_send![root, setSubviews: empty];
            let this = MainQueueObject(
                Retained::retain(this).expect("cannot retain a null preview controller"),
            );
            let url = SendableUrl(Retained::retain(url).expect("cannot retain a null URL"));
            let completion = PreviewCompletion(completion.copy());
            (this, url, completion)
        };

        dispatch_async(move || {
            // SAFETY: url is retained until this load completes.
            let media = unsafe { load_media(url.as_ptr()) };
            dispatch_async_main(move || {
                // SAFETY: this is retained and all AppKit view work happens on the main queue.
                unsafe {
                    let this = this.as_ptr_on_main();
                    match media {
                        Ok(media) => {
                            let size = preferred_content_size(media.size());
                            let root: *mut Object = msg_send![this, view];
                            let bounds: Rect = msg_send![root, bounds];
                            let view = create_image_view(bounds, &media);
                            let _: () = msg_send![&*view,
                                setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
                            ];
                            let subviews: *mut Object =
                                msg_send![class!(NSArray), arrayWithObject: view.as_ptr()];
                            let _: () = msg_send![root, setSubviews: subviews];
                            let _: () = msg_send![this, setPreferredContentSize: size];
                            completion.call(null_mut());
                        }
                        Err(description) => {
                            let error =
                                make_error(ns_string!("nl.bplaat.ImageView.Preview"), &description);
                            completion.call(error);
                        }
                    }
                }
            });
        });
    }
}

fn main() {
    let preview_class = PreviewViewController::class();
    // SAFETY: QuickLookUI provides the process-lifetime QLPreviewingController protocol, and
    // preview_class is the registered Objective-C class used as the extension principal class.
    unsafe {
        let protocol = AnyProtocol::get(c"QLPreviewingController")
            .expect("QLPreviewingController is unavailable");
        assert!(
            class_addProtocol(preview_class.cast::<AnyClass>(), protocol).as_bool(),
            "failed to adopt QLPreviewingController"
        );
    }
    extension_main();
}
