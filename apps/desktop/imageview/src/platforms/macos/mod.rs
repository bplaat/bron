/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The AppKit platform: the media view, folder browsing like Finder, the recent documents menu
//! and printing.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::path::Path;
use std::ptr::null_mut;

use block2::Block;
use bwindow::{EventLoopProxy, NativeViewHandle, Theme, Window, WindowAttachment};
use imageview_core::Media;
use imageview_core::appkit::{Point, Rect, Size, create_media_view, ns_string};
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

pub(crate) use self::browse::neighbour_path;
use self::headers::*;
use self::toolbar::install_toolbar;
use self::view::{View, rect};
use crate::AppEvent;
use crate::viewport::Viewport;

mod browse;
mod headers;
mod toolbar;
mod view;

/// The smallest window.
pub(crate) const MIN_SIZE: (f32, f32) = (240.0, 180.0);

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
                send(AppEvent::Open(vec![path]));
            }
            // The application shows the file in its own window, not in an NSDocument.
            completion.call((null_mut(), Bool::NO, null_mut()));
        }
    }
);

// Sends an event to the application.
fn send(event: AppEvent) {
    PROXY.with_borrow(|proxy| {
        if let Some(proxy) = proxy {
            _ = proxy.send_user_event(event);
        }
    });
}

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

/// A native view that zooms and pans media.
pub(crate) struct MediaView {
    view: Retained<View>,
    // The loading indicator, shown until the media is decoded.
    spinner: Option<Retained<Object>>,
    _toolbar: Retained<Object>,
    _attachment: WindowAttachment,
}

impl MediaView {
    /// Fills the window with the media zoomed to fit, or a loading indicator until the media
    /// is set.
    pub(crate) fn new(window: &Window, media: Option<Media>, theme: Theme) -> Self {
        let loading = media.is_none();
        let attachment = window.attach_content().expect("window has no content yet");
        let size = window.size();
        let view = View::new(
            rect(0.0, 0.0, f64::from(size.width), f64::from(size.height)),
            media,
        );
        view.set_dark(theme == Theme::Dark);
        // SAFETY: The attachment keeps the window that retains the view open while this view
        // lives.
        let toolbar = unsafe {
            attachment.set_native_view(NativeViewHandle::AppKit(Retained::as_ptr(&view) as _));
            let window: *mut Object = msg_send![&*view, window];
            // The unified toolbar changes the content size, so the window gets the size it was
            // built with back, keeping its top left corner in place.
            let frame: Rect = msg_send![window, frame];
            let toolbar = install_toolbar(window);
            let content_size = Size {
                width: f64::from(size.width),
                height: f64::from(size.height),
            };
            let _: () = msg_send![window, setContentSize: content_size];
            let top_left = Point {
                x: frame.origin.x,
                y: frame.origin.y + frame.size.height,
            };
            let _: () = msg_send![window, setFrameTopLeftPoint: top_left];
            // Enable full screen, code-created windows only zoom by default.
            let behavior: u64 = msg_send![window, collectionBehavior];
            let _: () = msg_send![window,
                setCollectionBehavior: behavior | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_PRIMARY
            ];
            toolbar
        };
        let spinner = if loading {
            Some(add_spinner(&view))
        } else {
            show_size(&view);
            None
        };
        // The view has its size now.
        view.update(Viewport::zoom_to_fit);
        view.advance_frame();
        Self {
            view,
            spinner,
            _toolbar: toolbar,
            _attachment: attachment,
        }
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        self.view
            .update(|viewport| viewport.set_media(media, zoom_to_fit));
        self.view.advance_frame();
        show_size(&self.view);
        if let Some(spinner) = self.spinner.take() {
            // SAFETY: The spinner is a live subview of the view.
            unsafe {
                let _: () = msg_send![&*spinner, stopAnimation: null_mut::<Object>()];
                let _: () = msg_send![&*spinner, removeFromSuperview];
            }
        }
    }

    /// Magnifies by `factor`, keeping the middle of the visible area in place.
    pub(crate) fn zoom_by(&self, factor: f64) {
        self.view.update(|viewport| viewport.zoom_by(factor, None));
    }

    /// Shows the media at its natural size.
    pub(crate) fn actual_size(&self) {
        self.view.update(Viewport::actual_size);
    }

    /// Shows all of the media.
    pub(crate) fn zoom_to_fit(&self) {
        self.view.update(Viewport::zoom_to_fit);
    }

    /// Follows the light or dark theme of the window.
    pub(crate) fn theme_changed(&self, theme: Theme) {
        self.view.set_dark(theme == Theme::Dark);
    }

    /// Prints the media at its natural size, scaled to fit one page.
    pub(crate) fn print(&self) {
        let media = self.view.media();
        let (width, height) = media.size();
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
                let view = create_media_view(rect(0.0, 0.0, width, height), media);
                let operation: *mut Object = msg_send![class!(NSPrintOperation),
                    printOperationWithView: view.as_ptr(),
                    printInfo: print_info.as_ptr()
                ];
                let window: *mut Object = msg_send![&*self.view, window];
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

// Shows a spinning progress indicator in the middle of the view, which keeps it there.
fn add_spinner(view: &View) -> Retained<Object> {
    const SIZE: f64 = 32.0;
    // SAFETY: The indicator is a valid AppKit view that the view retains as its subview.
    unsafe {
        let bounds: Rect = msg_send![view, bounds];
        let spinner: Allocated<Object> = msg_send![class!(NSProgressIndicator), alloc];
        let spinner: Retained<Object> = msg_send![spinner, initWithFrame: rect(
            ((bounds.size.width - SIZE) / 2.0).round(),
            ((bounds.size.height - SIZE) / 2.0).round(),
            SIZE,
            SIZE
        )];
        let _: () = msg_send![&*spinner, setStyle: NS_PROGRESS_INDICATOR_STYLE_SPINNING];
        let _: () = msg_send![&*spinner, setAutoresizingMask: NS_VIEW_CENTERED];
        let _: () = msg_send![view, addSubview: &*spinner];
        let _: () = msg_send![&*spinner, startAnimation: null_mut::<Object>()];
        spinner
    }
}

// Shows the size of the media as the subtitle of the window.
fn show_size(view: &View) {
    let (width, height) = view.media().size();
    let subtitle = format!("{} \u{d7} {}", width.round() as i64, height.round() as i64);
    // SAFETY: The view is in its window and the subtitle string is autoreleased.
    unsafe {
        let window: *mut Object = msg_send![view, window];
        let _: () = msg_send![window, setSubtitle: ns_string(&subtitle)];
    }
}

impl Drop for MediaView {
    fn drop(&mut self) {
        self.view.stop();
    }
}
