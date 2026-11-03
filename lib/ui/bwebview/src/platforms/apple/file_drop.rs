/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bwindow::ffi::{file_drag_operation, perform_file_drop};
use objc2::define_class;
use objc2::runtime::{AnyObject as Object, Bool};

define_class!(
    #[unsafe(super(WKWebView))]
    #[name = "BWebviewDroppableWebview"]
    struct DroppableWebview;

    impl DroppableWebview {
        #[unsafe(method(draggingEntered:))]
        fn _dragging_entered(&self, sender: *mut Object) -> u64 {
            unsafe { file_drag_operation(sender) }
        }

        #[unsafe(method(draggingUpdated:))]
        fn _dragging_updated(&self, sender: *mut Object) -> u64 {
            unsafe { file_drag_operation(sender) }
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
