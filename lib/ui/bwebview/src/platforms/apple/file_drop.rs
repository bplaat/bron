/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bwindow::ffi::perform_file_drop;
use objc2::define_class;
use objc2::runtime::{AnyObject as Object, Bool};

use super::headers::*;
define_class!(
    #[unsafe(super(WKWebView))]
    #[name = "BWebviewDroppableWebview"]
    struct DroppableWebview;

    impl DroppableWebview {
        #[unsafe(method(draggingEntered:))]
        const fn _dragging_entered(&self, _: *mut Object) -> u64 {
            NS_DRAG_OPERATION_COPY
        }

        #[unsafe(method(draggingUpdated:))]
        const fn _dragging_updated(&self, _: *mut Object) -> u64 {
            NS_DRAG_OPERATION_COPY
        }

        #[unsafe(method(prepareForDragOperation:))]
        const fn _prepare_for_drag_operation(&self, _: *mut Object) -> Bool {
            Bool::YES
        }

        #[unsafe(method(performDragOperation:))]
        fn _perform_drag_operation(&self, sender: *mut Object) -> Bool {
            unsafe { perform_file_drop(sender) }
        }
    }
);

/// A WKWebView subclass that reports native file drags instead of navigating to them
pub(super) fn droppable_webview_class() -> *mut Object {
    DroppableWebview::class()
}
