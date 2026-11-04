/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The AppKit platform: the media view, folder browsing like Finder, the recent documents menu
//! and printing.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use block2::Block;
use bwindow::{EventLoopProxy, NativeViewHandle, Theme, Window, WindowAttachment, WindowId};
use imageview_core::Media;
use imageview_core::appkit::{Point, Rect, Size, create_media_view, ns_string};
use imageview_core::edit::{Control, Selection, Shape, Tool};
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject as Object, Bool, Sel};
use objc2::{class, define_class, msg_send, sel};

pub(crate) use self::browse::neighbour_path;
use self::headers::*;
use self::popover::Popover;
use self::toolbar::{Mode, Toolbars};
use self::view::{View, rect};
use crate::AppEvent;
use crate::viewport::Viewport;

mod browse;
mod headers;
mod popover;
mod save;

pub(crate) use self::save::{SaveChoice, SaveRequest};
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
                let path = PathBuf::from(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(bytes));
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
    toolbars: Toolbars,
    // The popover of the open edit tool.
    popover: Option<Popover>,
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
        let toolbars = unsafe {
            attachment.set_native_view(NativeViewHandle::AppKit(Retained::as_ptr(&view) as _));
            let window: *mut Object = msg_send![&*view, window];
            // The unified toolbar changes the content size, so the window gets the size it was
            // built with back, keeping its top left corner in place.
            let frame: Rect = msg_send![window, frame];
            let toolbars = Toolbars::install(window);
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
            // The view has focus, so the Copy and Select All menu items reach it.
            let _: Bool = msg_send![window, makeFirstResponder: &*view];
            toolbars
        };
        let spinner = if loading {
            Some(add_spinner(&view))
        } else {
            show_size(&view);
            None
        };
        // Only still bitmaps can be edited, the loading media has no frames.
        toolbars.set_editable(view.media().is_still());
        // The view has its size now.
        view.update(Viewport::zoom_to_fit);
        view.advance_frame();
        Self {
            view,
            spinner,
            toolbars,
            popover: None,
            _attachment: attachment,
        }
    }

    /// The shown media.
    pub(crate) fn media(&self) -> Media {
        self.view.media()
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        // Vectors and animations cannot be edited, only saved as bitmaps.
        self.toolbars.set_editable(media.is_still());
        self.view
            .update(|viewport| viewport.set_media(media, zoom_to_fit));
        self.view.advance_frame();
        show_size(&self.view);
        if let Some(spinner) = self.spinner.take() {
            remove_spinner(&spinner);
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

    fn window(&self) -> *mut Object {
        // SAFETY: The view is in its window while this view lives.
        unsafe { msg_send![&*self.view, window] }
    }

    /// Places the window in the middle of its screen, like new windows.
    pub(crate) fn center_window(&self) {
        // SAFETY: The window is a live NSWindow.
        unsafe {
            let _: () = msg_send![self.window(), center];
        }
    }

    /// Shows the edit toolbar, or the view toolbar again.
    pub(crate) fn set_editing(&mut self, editing: bool) {
        self.close_tool();
        let mode = if editing { Mode::Edit } else { Mode::View };
        self.toolbars.show(self.window(), mode);
    }

    /// Marks the window as having unsaved changes, with a dot in its close button.
    pub(crate) fn set_edited(&self, edited: bool) {
        // SAFETY: The window is a live NSWindow.
        unsafe {
            let _: () = msg_send![self.window(), setDocumentEdited: objc_bool(edited)];
        }
    }

    /// Shows the popover with controls of a tool below the toolbar button of `action`, and
    /// the crop rectangle for a crop. Changes of the controls go to `window`, reset goes back
    /// to the defaults.
    pub(crate) fn show_tool(
        &mut self,
        window: WindowId,
        tool: Tool,
        controls: &[Control],
        values: &[f32],
        defaults: &[f32],
        action: &str,
    ) {
        self.close_tool();
        if tool == Tool::Crop {
            self.view.set_cropping(true);
        }
        let popover = Popover::new(window, tool, controls, values, defaults);
        popover.show(
            self.toolbars.edit_item(action),
            Retained::as_ptr(&self.view) as *mut Object,
        );
        self.popover = Some(popover);
    }

    /// Shows changed values of the open tool.
    pub(crate) fn set_tool_values(&self, values: &[f32]) {
        if let Some(popover) = &self.popover {
            popover.set_values(values);
        }
    }

    /// Closes the popover and crop rectangle of the open tool.
    pub(crate) fn close_tool(&mut self) {
        if self.popover.take().is_some() {
            self.view.set_cropping(false);
            // SAFETY: The window is a live NSWindow and the view is in it.
            unsafe {
                let _: Bool = msg_send![self.window(), makeFirstResponder: &*self.view];
            }
        }
    }

    /// Makes drags draw selections of a shape, or selects the whole image again, shown as on
    /// in the toolbar.
    pub(crate) fn set_selecting(&self, shape: Option<Shape>) {
        self.view.set_selecting(shape);
        self.toolbars.set_toggled(match shape {
            None => "select.whole",
            Some(Shape::Rectangle) => "select.rectangle",
            Some(Shape::Ellipse) => "select.ellipse",
        });
    }

    /// The selected part of the media.
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.view.selection()
    }

    /// Selects everything outside the selection instead.
    pub(crate) fn invert_selection(&self) {
        self.view.invert_selection();
    }

    /// Asks in a sheet where to save the image, in which format and with which JPEG quality,
    /// and at which size for a vector.
    pub(crate) fn ask_save(&self, request: &SaveRequest<'_>) -> Option<SaveChoice> {
        save::save_panel(self.window(), request)
    }

    /// Puts a PNG image on the clipboard.
    pub(crate) fn copy_png(&self, png: &[u8]) {
        autoreleasepool(|_| {
            // SAFETY: NSData copies the bytes, the pasteboard retains the autoreleased image.
            unsafe {
                let data: *mut Object = msg_send![class!(NSData),
                    dataWithBytes: png.as_ptr().cast::<std::ffi::c_void>(),
                    length: png.len()
                ];
                let image: Allocated<Object> = msg_send![class!(NSImage), alloc];
                let image: Option<Retained<Object>> = msg_send![image, initWithData: data];
                let Some(image) = image else {
                    return;
                };
                let pasteboard: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
                let _: isize = msg_send![pasteboard, clearContents];
                let objects: *mut Object = msg_send![class!(NSArray), arrayWithObject: &*image];
                let _: Bool = msg_send![pasteboard, writeObjects: objects];
            }
        });
    }

    /// Shows a preview of any size in the place of the image, or the image again.
    pub(crate) fn set_preview(&self, preview: Option<Media>) {
        self.view.set_preview(preview);
    }

    /// Places the crop rectangle at a position with a size in pixels.
    pub(crate) fn set_crop_rect(&self, rect: (u32, u32, u32, u32)) {
        self.view.set_crop_rect(rect);
    }

    /// The longest side of the view in device pixels, which previews do not need to exceed.
    pub(crate) fn longest_pixels(&self) -> u32 {
        self.view.longest_pixels()
    }

    /// Shows a spinning progress indicator while an edit is applied.
    pub(crate) fn set_busy(&mut self, busy: bool) {
        if busy && self.spinner.is_none() {
            self.spinner = Some(add_spinner(&self.view));
        } else if !busy && let Some(spinner) = self.spinner.take() {
            remove_spinner(&spinner);
        }
    }

    /// Makes the crop rectangle keep a width divided by height, or any shape.
    pub(crate) fn set_crop_aspect(&self, aspect: Option<f64>) {
        self.view.set_crop_aspect(aspect);
    }

    /// The crop rectangle in pixels.
    pub(crate) fn crop_rect(&self) -> (u32, u32, u32, u32) {
        self.view.crop_rect()
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
                    didRunSelector: Sel(std::ptr::null()),
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

// Stops and removes a progress indicator.
fn remove_spinner(spinner: &Object) {
    // SAFETY: The spinner is a live subview of the view.
    unsafe {
        let _: () = msg_send![spinner, stopAnimation: null_mut::<Object>()];
        let _: () = msg_send![spinner, removeFromSuperview];
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
        self.popover = None;
        self.view.stop();
    }
}
