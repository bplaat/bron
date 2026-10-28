/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The AppKit media view: a magnifying NSScrollView above a checkerboard, which shows media
//! drawn with bcanvas.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::path::Path;
use std::ptr::null_mut;

use block2::Block;
use bwindow::{EventLoopProxy, NativeViewHandle, Window, WindowAttachment};
use imageview_core::Media;
use imageview_core::appkit::{Point, Rect, Size, create_media_view, ns_string};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

pub(crate) use self::browse::neighbour_path;
use self::checkerboard::create_checkerboard_view;
use self::headers::*;
use self::scroll_view::{create_scroll_view, replace_document_view};
use crate::AppEvent;

mod browse;
mod checkerboard;
mod headers;
mod scroll_view;

thread_local! {
    static PROXY: RefCell<Option<EventLoopProxy<AppEvent>>> = const { RefCell::new(None) };
}

define_class!(
    // Opens the recent documents menu items in the application's own windows.
    #[unsafe(super(NSDocumentController))]
    #[name = "ImageViewDocumentController"]
    struct DocumentController;

    impl DocumentController {
        #[unsafe(method(openDocumentWithContentsOfURL:display:completionHandler:))]
        fn _open_document(
            &self,
            url: *mut Object,
            _display: Bool,
            completion: &Block<dyn Fn(*mut Object, Bool, *mut Object)>,
        ) {
            // SAFETY: AppKit passes a valid file URL.
            let path: *const std::ffi::c_char = unsafe { msg_send![url, fileSystemRepresentation] };
            if !path.is_null() {
                // SAFETY: NSURL returns a null-terminated representation that lives in this pool.
                let bytes = unsafe { std::ffi::CStr::from_ptr(path) }.to_bytes();
                let path = std::path::PathBuf::from(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(bytes));
                PROXY.with_borrow(|proxy| {
                    if let Some(proxy) = proxy {
                        _ = proxy.send_user_event(AppEvent::Open(vec![path]));
                    }
                });
            }
            // The application shows the file in its own window, not in an NSDocument.
            completion.call((null_mut(), Bool::NO, null_mut()));
        }
    }
);

/// Sets up the recent documents menu.
pub(crate) fn init(proxy: EventLoopProxy<AppEvent>) {
    PROXY.with_borrow_mut(|stored| *stored = Some(proxy));
    // SAFETY: The first document controller becomes the shared one for the process lifetime, and
    // every menu object is retained by the menu it is added to.
    unsafe {
        // Every image has its own window, so disable window tabbing.
        let _: () = msg_send![class!(NSWindow), setAllowsAutomaticWindowTabbing: Bool::NO];
        let controller: Retained<Object> = msg_send![DocumentController::class(), new];
        std::mem::forget(controller);

        let application: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let main_menu: *mut Object = msg_send![application, mainMenu];
        let file_item: *mut Object = msg_send![main_menu, itemWithTitle: ns_string!("File")];
        if file_item.is_null() {
            return;
        }
        let file_menu: *mut Object = msg_send![file_item, submenu];
        let open_index: isize = msg_send![file_menu, indexOfItemWithTitle: ns_string!("Open...")];
        let _: () = msg_send![file_menu,
            insertItem: open_recent_item().as_ptr(),
            atIndex: open_index + 1
        ];
    }
}

// Creates the named recent files submenu that `NSDocumentController` maintains.
fn open_recent_item() -> Retained<Object> {
    // SAFETY: The item retains the submenu, which retains its clear item.
    unsafe {
        let item: Retained<Object> = msg_send![class!(NSMenuItem), new];
        let _: () = msg_send![&*item, setTitle: ns_string!("Open Recent")];
        let menu: Retained<Object> = msg_send![class!(NSMenu), new];
        let _: () = msg_send![&*menu, setTitle: ns_string!("Open Recent")];
        let named: Bool = msg_send![&*menu, respondsToSelector: sel!(_setMenuName:)];
        if named.as_bool() {
            let _: () = msg_send![&*menu, _setMenuName: ns_string!("NSRecentDocumentsMenu")];
        }
        let clear: Retained<Object> = msg_send![class!(NSMenuItem), new];
        let _: () = msg_send![&*clear, setTitle: ns_string!("Clear Menu")];
        let _: () = msg_send![&*clear, setAction: sel!(clearRecentDocuments:)];
        let _: () = msg_send![&*menu, addItem: clear.as_ptr()];
        let _: () = msg_send![&*item, setSubmenu: menu.as_ptr()];
        item
    }
}

/// Adds the file to the recent documents menu.
pub(crate) fn note_recent_file(path: &Path) {
    autoreleasepool(|_| {
        // SAFETY: The URL is autoreleased and the shared document controller lives forever.
        unsafe {
            let url = browse::file_url(path);
            if url.is_null() {
                return;
            }
            let controller: *mut Object =
                msg_send![class!(NSDocumentController), sharedDocumentController];
            let _: () = msg_send![controller, noteNewRecentDocumentURL: url];
        }
    });
}

/// A native scroll view that zooms and pans media above a checkerboard.
pub(crate) struct MediaView {
    media: Media,
    scroll_view: Retained<Object>,
    _checkerboard: Retained<Object>,
    _attachment: WindowAttachment,
}

impl MediaView {
    /// Fills the window with the media, zoomed to fit.
    pub(crate) fn new(window: &Window, media: Media, _theme: bwindow::Theme) -> Self {
        let attachment = window.attach_content().expect("window has no content yet");
        let size = window.size();
        let frame = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: f64::from(size.width),
                height: f64::from(size.height),
            },
        };
        let checkerboard = create_checkerboard_view(frame);
        let media_view = create_media_view(media_frame(&media), media.clone());
        let scroll_view = create_scroll_view(frame, media_view.as_ptr());
        // SAFETY: The views are live and the checkerboard retains the scroll view, the attachment
        // keeps the window that retains the checkerboard open while this view lives.
        unsafe {
            let _: () = msg_send![&*scroll_view, zoomToFit];
            let _: () = msg_send![&*checkerboard, addSubview: scroll_view.as_ptr()];
            attachment.set_native_view(NativeViewHandle::AppKit(checkerboard.as_ptr().cast()));
            let window: *mut Object = msg_send![&*checkerboard, window];
            let _: Bool = msg_send![window, makeFirstResponder: scroll_view.as_ptr()];
            // Enable full screen, code-created windows only zoom by default.
            let behavior: u64 = msg_send![window, collectionBehavior];
            let _: () = msg_send![window,
                setCollectionBehavior: behavior | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_PRIMARY
            ];
        }
        Self {
            media,
            scroll_view,
            _checkerboard: checkerboard,
            _attachment: attachment,
        }
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        let media_view = create_media_view(media_frame(&media), media.clone());
        // SAFETY: The scroll view is live and retains the new media view.
        unsafe {
            replace_document_view(self.scroll_view.as_ptr(), media_view.as_ptr(), zoom_to_fit)
        };
        self.media = media;
    }

    /// Magnifies by `factor`, keeping the middle of the visible area in place.
    pub(crate) fn zoom_by(&self, factor: f64) {
        // SAFETY: The scroll view implements `zoomBy:` with a double.
        unsafe {
            let _: () = msg_send![&*self.scroll_view, zoomBy: factor];
        }
    }

    /// Shows the media at its natural size.
    pub(crate) fn actual_size(&self) {
        // SAFETY: The scroll view implements `setZoom:` with a double.
        unsafe {
            let _: () = msg_send![&*self.scroll_view, setZoom: 1.0f64];
        }
    }

    /// Shows all of the media.
    pub(crate) fn zoom_to_fit(&self) {
        // SAFETY: The scroll view implements `zoomToFit`.
        unsafe {
            let _: () = msg_send![&*self.scroll_view, zoomToFit];
        }
    }

    /// AppKit follows the theme itself.
    pub(crate) const fn theme_changed(&self, _theme: bwindow::Theme) {}

    /// Prints the media at its natural size, scaled to fit one page.
    pub(crate) fn print(&self) {
        autoreleasepool(|_| {
            // SAFETY: All objects are valid AppKit instances. The print info copy and the media
            // view are released after the operation that retains them has run.
            unsafe {
                let print_info: *mut Object = msg_send![class!(NSPrintInfo), sharedPrintInfo];
                let print_info: Retained<Object> = msg_send![print_info, copy];
                let _: () = msg_send![&*print_info,
                    setHorizontalPagination: NS_PRINTING_PAGINATION_MODE_FIT
                ];
                let _: () =
                    msg_send![&*print_info, setVerticalPagination: NS_PRINTING_PAGINATION_MODE_FIT];
                let _: () = msg_send![&*print_info, setHorizontallyCentered: Bool::YES];
                let _: () = msg_send![&*print_info, setVerticallyCentered: Bool::YES];
                let view = create_media_view(media_frame(&self.media), self.media.clone());
                let operation: *mut Object = msg_send![class!(NSPrintOperation),
                    printOperationWithView: view.as_ptr(),
                    printInfo: print_info.as_ptr()
                ];
                let window: *mut Object = msg_send![&*self.scroll_view, window];
                let _: () = msg_send![operation,
                    runOperationModalForWindow: window,
                    delegate: null_mut::<Object>(),
                    didRunSelector: null_mut::<std::ffi::c_void>(),
                    contextInfo: null_mut::<std::ffi::c_void>()
                ];
            }
        });
    }
}

fn media_frame(media: &Media) -> Rect {
    let (width, height) = media.size();
    Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: Size { width, height },
    }
}
