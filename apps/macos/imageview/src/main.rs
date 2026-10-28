/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ptr::null_mut;

use block2::RcBlock;
use imageview_appkit::{
    Image, NS_VIEW_HEIGHT_SIZABLE, NS_VIEW_WIDTH_SIZABLE, OwnedString, Point, Rect, Size,
    create_image_view, decode_image, decode_image_data, dispatch_async, dispatch_async_main,
    make_error, ns_string, preferred_content_size,
};
use objc2::ffi::{class_addMethod, object_getClass};
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use crate::browse::{file_url, neighbour_path, url_path};
use crate::checkerboard::create_checkerboard_view;
use crate::headers::*;
use crate::scroll_view::create_scroll_view;
use crate::window_controller::{create_window_controller, show_media};

mod browse;
mod checkerboard;
mod headers;
mod scroll_view;
mod window_controller;

struct DocumentIvars {
    media: RefCell<Option<DecodedMedia>>,
    content_version: Cell<Option<ContentVersion>>,
    browse_generation: Cell<u64>,
    reload_generation: Cell<u64>,
}

impl DocumentIvars {
    const fn new() -> Self {
        Self {
            media: RefCell::new(None),
            content_version: Cell::new(None),
            browse_generation: Cell::new(0),
            reload_generation: Cell::new(0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ContentVersion {
    length: usize,
    hash: u64,
}

type DecodedMedia = Image;

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

define_class!(
    #[unsafe(super(NSDocument))]
    #[name = "ImageViewDocument"]
    #[ivars = DocumentIvars]
    struct Document;

    impl Document {
        #[unsafe(method_id(init))]
        fn _init(this: Allocated<Self>) -> Option<Retained<Self>> {
            // SAFETY: `this` is allocated as Document and NSObject implements `init` with this
            // return type.
            unsafe { msg_send![super(this.set_ivars(DocumentIvars::new())), init] }
        }

        #[unsafe(method(readFromData:ofType:error:))]
        fn _read_from_data(
            &self,
            data: *mut Object,
            _: *mut Object,
            error: *mut c_void,
        ) -> Bool {
            self.read_from_data(data, error)
        }

        #[unsafe(method(makeWindowControllers))]
        fn _make_window_controllers(&self) {
            self.make_window_controllers();
        }

        #[unsafe(method(presentedItemDidChange))]
        fn _presented_item_did_change(&self) {
            self.reload_presented_item();
        }

        #[unsafe(method(nextImage:))]
        fn _next_image(&self, _: *mut Object) {
            self.show_sibling(1);
        }

        #[unsafe(method(previousImage:))]
        fn _previous_image(&self, _: *mut Object) {
            self.show_sibling(-1);
        }

        #[unsafe(method(printOperationWithSettings:error:))]
        fn _print_operation_with_settings(
            &self,
            settings: *mut Object,
            _: *mut c_void,
        ) -> *mut Object {
            self.print_operation(settings)
        }
    }
);

impl Document {
    // Reloads the URL watched by NSDocument after another process changes it.
    fn reload_presented_item(&self) {
        let this = self as *const Self as *mut Object;
        let retained = MainQueueObject(
            // SAFETY: NSFilePresenter keeps the document alive for this callback.
            unsafe { Retained::retain(this) }.expect("cannot retain a null document"),
        );
        // File-presenter callbacks arrive on a private queue: capture the URL on the main queue, then
        // read and decode on a worker.
        dispatch_async_main(move || {
            // SAFETY: retained owns the document until start_presented_item_reload moves that
            // ownership into the worker continuation.
            unsafe {
                let this = retained.as_ptr_on_main();
                let document = &*this.cast::<Document>();
                document.start_presented_item_reload(retained);
            }
        });
    }

    // Reads and decodes a changed presented item without blocking AppKit's main thread.
    unsafe fn start_presented_item_reload(&self, retained_document: MainQueueObject) {
        // SAFETY: This runs on the main queue with a retained document.
        let prepared = unsafe {
            let this = self as *const Self as *mut Object;
            let url: *mut Object = msg_send![this, fileURL];
            if url.is_null() {
                None
            } else {
                let generation = self.ivars().reload_generation.get().wrapping_add(1);
                self.ivars().reload_generation.set(generation);
                let url = SendableUrl(
                    Retained::retain(url).expect("cannot retain a null presented item URL"),
                );
                Some((url, generation))
            }
        };
        let Some((url, generation)) = prepared else {
            return;
        };

        dispatch_async(move || {
            // SAFETY: The URL remains retained throughout this coordinated load.
            let result = unsafe { load_document(url.as_ptr()) };
            dispatch_async_main(move || {
                // SAFETY: The document and URL remain retained for this main-queue continuation.
                unsafe {
                    let this = retained_document.as_ptr_on_main();
                    let document = &*this.cast::<Document>();
                    if document.ivars().reload_generation.get() != generation {
                        return;
                    }
                    let current_url: *mut Object = msg_send![this, fileURL];
                    let same_url: Bool = msg_send![url.as_ptr(), isEqual: current_url];
                    if !same_url.as_bool() {
                        return;
                    }
                    let Ok((media, _, version)) = result else {
                        return;
                    };
                    if document.ivars().content_version.get() == Some(version) {
                        return;
                    }
                    document.install_media(media, version);
                    document.refresh_windows(false);
                }
            });
        });
    }

    fn read_from_data(&self, data: *mut Object, error_out: *mut c_void) -> Bool {
        // SAFETY: AppKit supplied a valid NSData for the duration of this call.
        let version = content_version(unsafe { document_data_bytes(data) });
        if self.ivars().content_version.get() == Some(version) {
            return Bool::YES;
        }
        // SAFETY: AppKit supplied a valid NSData for the duration of this call.
        match unsafe { decode_document(data) } {
            Ok(media) => {
                let refresh = self.ivars().media.borrow().is_some();
                self.install_media(media, version);
                if refresh {
                    // Views must be replaced on the main queue, the retained document keeps the media alive
                    // until then.
                    let this = self as *const Self as *mut Object;
                    let retained = MainQueueObject(
                        // SAFETY: this comes from a live shared reference to the document.
                        unsafe { Retained::retain(this) }.expect("cannot retain a null document"),
                    );
                    // SAFETY: retained owns the document until this main-queue callback ends.
                    dispatch_async_main(move || unsafe {
                        let this = retained.as_ptr_on_main();
                        let document = &*this.cast::<Document>();
                        document.refresh_windows(false);
                    });
                }
                Bool::YES
            }
            Err(error) => {
                set_error(error_out, &error);
                Bool::NO
            }
        }
    }

    fn install_media(&self, media: DecodedMedia, version: ContentVersion) {
        self.ivars().media.replace(Some(media));
        self.ivars().content_version.set(Some(version));
    }

    fn next_browse_generation(&self) -> u64 {
        let generation = self.ivars().browse_generation.get().wrapping_add(1);
        self.ivars().browse_generation.set(generation);
        generation
    }

    fn is_current_browse(&self, generation: u64) -> bool {
        self.ivars().browse_generation.get() == generation
    }

    fn show_sibling(&self, offset: isize) {
        // SAFETY: AppKit calls actions on the main thread. The retained document remains live
        // until the final main-queue continuation releases it.
        let (path, generation, retained, document_class) = unsafe {
            let this = self as *const Self as *mut Object;
            let url: *mut Object = msg_send![this, fileURL];
            let Some(path) = (!url.is_null()).then(|| url_path(url)).flatten() else {
                return;
            };
            let generation = self.next_browse_generation();
            let retained =
                MainQueueObject(Retained::retain(this).expect("cannot retain a null document"));
            (path, generation, retained, Self::class() as usize)
        };

        dispatch_async(move || {
            // SAFETY: The document class is registered for the process lifetime.
            let neighbour = unsafe { neighbour_path(&path, offset, document_class as *mut Object) };
            dispatch_async_main(move || {
                // SAFETY: retained owns a live document until this continuation releases it.
                unsafe {
                    let this = retained.as_ptr_on_main();
                    let document = &*this.cast::<Document>();
                    if !document.is_current_browse(generation) {
                        return;
                    }
                    let Some(path) = neighbour else {
                        return;
                    };
                    document.load_sibling(path, generation, retained);
                }
            });
        });
    }

    // Starts loading a neighbouring file after its folder scan has completed.
    unsafe fn load_sibling(
        &self,
        path: std::path::PathBuf,
        generation: u64,
        retained_document: MainQueueObject,
    ) {
        let error_domain = ns_string!("nl.bplaat.ImageView") as usize;
        // SAFETY: This runs on the main queue with a retained document from show_sibling.
        let prepared = unsafe {
            let url = file_url(&path);
            if url.is_null() {
                None
            } else {
                let controller: *mut Object =
                    msg_send![class!(NSDocumentController), sharedDocumentController];
                let open: *mut Object = msg_send![controller, documentForURL: url];
                if !open.is_null() {
                    let _: () = msg_send![open, showWindows];
                    None
                } else {
                    let url = SendableUrl(
                        Retained::retain(url).expect("cannot retain a null document URL"),
                    );
                    Some((url, retained_document))
                }
            }
        };
        let Some((url, retained_document)) = prepared else {
            return;
        };

        dispatch_async(move || {
            // SAFETY: The URL is retained until the main-queue continuation releases it.
            let result = unsafe { load_document(url.as_ptr()) };
            dispatch_async_main(move || {
                // SAFETY: All Objective-C objects were retained for this continuation.
                unsafe {
                    let this = retained_document.as_ptr_on_main();
                    let document = &*this.cast::<Document>();
                    let url = url.as_ptr();
                    if document.is_current_browse(generation) {
                        match result {
                            Ok((media, kind, version)) => {
                                document.install_media(media, version);
                                let _: () = msg_send![this, setFileURL: url];
                                let _: () = msg_send![this, setFileType: kind.as_ptr()];
                                let controller: *mut Object = msg_send![
                                    class!(NSDocumentController),
                                    sharedDocumentController
                                ];
                                let _: () = msg_send![controller, noteNewRecentDocumentURL: url];
                                document.refresh_windows(true);
                            }
                            Err(description) => {
                                let error = make_error(error_domain as *mut Object, &description);
                                let _: () = msg_send![this, presentError: error];
                            }
                        }
                    }
                }
            });
        });
    }

    // Returns the natural size of the loaded media, or `None` when the document is empty.
    fn media_size(&self) -> Option<Size> {
        self.ivars().media.borrow().as_ref().map(Image::size)
    }

    // Creates an owned view that draws the loaded media inside `frame`.
    //
    // The returned view owns one retain count.
    fn create_media_view(&self, frame: Rect) -> Retained<Object> {
        let media = self.ivars().media.borrow();
        let image = media
            .as_ref()
            .expect("cannot create a view without loaded media");
        create_image_view(frame, image)
    }

    // Returns the size the window title shows, which is the stored size of a bitmap.
    fn title_size(&self, media_size: Size) -> Size {
        let media = self.ivars().media.borrow();
        media
            .as_ref()
            .and_then(Image::pixel_size)
            .unwrap_or(media_size)
    }

    // Shows the media this document holds now in the windows it opened before.
    fn refresh_windows(&self, zoom_to_fit: bool) {
        let Some(media_size) = self.media_size() else {
            return;
        };
        let title_size = self.title_size(media_size);
        // SAFETY: The document owns its window controllers, and each media view is released after
        // the scroll view of a window retains it.
        unsafe {
            let this = self as *const Self as *mut Object;
            let controllers: *mut Object = msg_send![this, windowControllers];
            let count: usize = msg_send![controllers, count];
            for index in 0..count {
                let controller: *mut Object = msg_send![controllers, objectAtIndex: index];
                let view = self.create_media_view(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: media_size,
                });
                show_media(controller, view.as_ptr(), title_size, zoom_to_fit);
            }
        }
    }

    fn make_window_controllers(&self) {
        let Some(media_size) = self.media_size() else {
            return;
        };
        let title_size = self.title_size(media_size);

        // SAFETY: All objects are valid AppKit instances, selectors use their documented ABI,
        // and ownership is balanced after the document retains its window controller.
        unsafe {
            let content_size = preferred_content_size(media_size);
            let rect = Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: content_size,
            };
            let style = NS_WINDOW_STYLE_MASK_TITLED
                | NS_WINDOW_STYLE_MASK_CLOSABLE
                | NS_WINDOW_STYLE_MASK_MINIATURIZABLE
                | NS_WINDOW_STYLE_MASK_RESIZABLE;
            let window: Allocated<Object> = msg_send![class!(NSWindow), alloc];
            let window: Retained<Object> = msg_send![window,
                initWithContentRect: rect,
                styleMask: style,
                backing: NS_BACKING_STORE_BUFFERED,
                defer: Bool::NO
            ];
            let _: () =
                msg_send![&*window, setContentMinSize: Size { width: 240.0, height: 180.0 }];
            // Enable full screen, code-created windows only zoom by default.
            let behavior: u64 = msg_send![&*window, collectionBehavior];
            let _: () = msg_send![&*window,
                setCollectionBehavior: behavior | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_PRIMARY
            ];
            let _: () = msg_send![&*window, center];

            // The scroll view magnifies the media, so vector formats redraw sharply when zooming.
            let checkerboard = create_checkerboard_view(rect);
            let media_view = self.create_media_view(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: media_size,
            });
            let scroll_view = create_scroll_view(rect, media_view.as_ptr());
            // Open windows zoomed to fit.
            let _: () = msg_send![&*scroll_view, zoomToFit];
            let _: () = msg_send![&*checkerboard, addSubview: scroll_view.as_ptr()];
            let _: () = msg_send![&*checkerboard,
                setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
            ];
            let _: () = msg_send![&*window, setContentView: checkerboard.as_ptr()];

            let controller = create_window_controller(window.as_ptr(), title_size);
            let this = self as *const Self as *mut Object;
            let _: () = msg_send![this, addWindowController: controller.as_ptr()];
        }
    }

    // Creates a print operation that draws the media at its natural size, scaled to fit one page.
    fn print_operation(&self, settings: *mut Object) -> *mut Object {
        let Some(media_size) = self.media_size() else {
            return null_mut();
        };
        // SAFETY: All objects are valid AppKit instances. The print info copy and the media view
        // are released after the returned operation retains them.
        unsafe {
            let this = self as *const Self as *mut Object;
            let print_info: *mut Object = msg_send![this, printInfo];
            let print_info: Retained<Object> = msg_send![print_info, copy];
            let attributes: *mut Object = msg_send![&*print_info, dictionary];
            let _: () = msg_send![attributes, addEntriesFromDictionary: settings];
            let _: () = msg_send![&*print_info,
                setHorizontalPagination: NS_PRINTING_PAGINATION_MODE_FIT
            ];
            let _: () =
                msg_send![&*print_info, setVerticalPagination: NS_PRINTING_PAGINATION_MODE_FIT];
            let _: () = msg_send![&*print_info, setHorizontallyCentered: Bool::YES];
            let _: () = msg_send![&*print_info, setVerticallyCentered: Bool::YES];

            let view = self.create_media_view(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: media_size,
            });
            let operation: *mut Object = msg_send![class!(NSPrintOperation),
                printOperationWithView: view.as_ptr(),
                printInfo: print_info.as_ptr()
            ];
            operation
        }
    }
}

// Decodes document data without creating any views or touching window state.
//
// # Safety
//
// `data` must point to a valid `NSData` for the duration of this call.
unsafe fn decode_document(data: *mut Object) -> Result<DecodedMedia, String> {
    // SAFETY: data remains live for the initializer, which owns what it needs after this call.
    unsafe { decode_image_data(data) }
}

// Returns the immutable byte buffer owned by an NSData object.
//
// # Safety
//
// `data` must point to a valid `NSData`, and the returned slice must not outlive it.
unsafe fn document_data_bytes<'a>(data: *mut Object) -> &'a [u8] {
    // SAFETY: The caller keeps NSData and its immutable byte buffer alive for the returned slice.
    unsafe {
        let length: usize = msg_send![data, length];
        let bytes: *const c_void = msg_send![data, bytes];
        if length == 0 {
            &[]
        } else {
            assert!(!bytes.is_null(), "non-empty NSData returned null bytes");
            std::slice::from_raw_parts(bytes.cast::<u8>(), length)
        }
    }
}

// Identifies file contents so presenter notifications without a content change are ignored.
fn content_version(bytes: &[u8]) -> ContentVersion {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    ContentVersion {
        length: bytes.len(),
        hash: hasher.finish(),
    }
}

// Decodes bytes read by Rust without creating views or touching window state.
fn decode_document_bytes(bytes: Vec<u8>) -> Result<DecodedMedia, String> {
    decode_image(bytes)
}

// Reads and decodes a document URL on a background queue.
//
// # Safety
//
// `url` must point to a retained `NSURL` for the duration of this call.
unsafe fn load_document(
    url: *mut Object,
) -> Result<(DecodedMedia, OwnedString, ContentVersion), String> {
    let result = std::sync::Arc::new(std::sync::Mutex::new(None));
    let accessor_result = result.clone();
    let accessor = RcBlock::new::<*mut Object>(move |coordinated_url| {
        // SAFETY: NSFileCoordinator supplies a live coordinated file URL for this synchronous
        // accessor invocation. Catching a panic here prevents unwinding across the Objective-C
        // block ABI.
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            decode_document_url(coordinated_url)
        }))
        .unwrap_or_else(|_| Err(String::from("Could not decode the image")));
        *accessor_result
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(decoded);
    });

    // SAFETY: url is retained by the caller. The coordinator invokes the copied accessor before
    // returning, and it coordinates the read against file presenters and writers.
    unsafe {
        let scoped: Bool = msg_send![url, startAccessingSecurityScopedResource];
        let coordinator: Allocated<Object> = msg_send![class!(NSFileCoordinator), alloc];
        let coordinator: Retained<Object> = msg_send![coordinator,
            initWithFilePresenter: null_mut::<Object>()
        ];
        let _: () = msg_send![&*coordinator,
            coordinateReadingItemAtURL: url,
            options: 0usize,
            error: null_mut::<c_void>(),
            byAccessor: &*accessor
        ];
        if scoped.as_bool() {
            let _: () = msg_send![url, stopAccessingSecurityScopedResource];
        }
    }

    result
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .unwrap_or_else(|| Err(String::from("Could not coordinate reading the image")))
}

// Reads and decodes a URL supplied by `NSFileCoordinator`.
//
// # Safety
//
// `url` must be a valid coordinated file URL for this call.
unsafe fn decode_document_url(
    url: *mut Object,
) -> Result<(DecodedMedia, OwnedString, ContentVersion), String> {
    // SAFETY: The caller supplies a live coordinated file URL.
    let path = unsafe { url_path(url) }.ok_or_else(|| String::from("Could not read the image"))?;
    let bytes = std::fs::read(&path).map_err(|_| String::from("Could not read the image"))?;
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| String::from("Unsupported image type"))?;
    // SAFETY: UTType and NSString are immutable and safe on this worker.
    let identifier = unsafe { type_identifier(extension) }?;
    let version = content_version(&bytes);
    decode_document_bytes(bytes).map(|media| (media, identifier, version))
}

// Returns the type identifier ImageView declares for an extension.
//
// # Safety
//
// This calls thread-safe immutable Uniform Type Identifier APIs.
unsafe fn type_identifier(extension: &str) -> Result<OwnedString, String> {
    let declared = if extension.eq_ignore_ascii_case("apng") {
        Some("org.libpng.apng")
    } else if extension.eq_ignore_ascii_case("bmp") {
        Some("com.microsoft.bmp")
    } else if extension.eq_ignore_ascii_case("jfif") {
        Some("org.jpeg.jfif")
    } else if extension.eq_ignore_ascii_case("qoi") {
        Some("org.qoiformat.qoi")
    } else if extension.eq_ignore_ascii_case("tvg") {
        Some("org.tinyvg.tvg")
    } else {
        None
    };
    if let Some(identifier) = declared {
        return Ok(OwnedString::new(identifier));
    }

    // SAFETY: UTType returns immutable autoreleased objects, and OwnedString retains the result.
    unsafe {
        let kind: *mut Object = msg_send![class!(UTType),
            typeWithFilenameExtension: ns_string(extension)
        ];
        if kind.is_null() {
            return Err(String::from("Unsupported image type"));
        }
        let identifier: *mut Object = msg_send![kind, identifier];
        Ok(OwnedString::retain(identifier))
    }
}

const extern "C-unwind" fn can_concurrently_read_documents(
    _: *mut Object,
    _: objc2::runtime::Sel,
    _: *mut Object,
) -> Bool {
    Bool::YES
}

// Tells `NSDocumentController` that document decoding is safe on its background queue.
unsafe fn enable_concurrent_document_reading() {
    // SAFETY: Document is registered, object_getClass returns its metaclass, and the function
    // signature matches +canConcurrentlyReadDocumentsOfType: (BOOL, Class, SEL, NSString *).
    unsafe {
        let metaclass = object_getClass(Document::class());
        let encoding = if cfg!(target_arch = "aarch64") {
            c"B@:@"
        } else {
            c"c@:@"
        };
        let added = class_addMethod(
            metaclass,
            sel!(canConcurrentlyReadDocumentsOfType:).0,
            can_concurrently_read_documents as *const c_void,
            encoding.as_ptr(),
        );
        assert!(
            added.as_bool(),
            "failed to enable concurrent document reading"
        );
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ImageViewAppDelegate"]
    struct AppDelegate;

    impl AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn _did_finish_launching(&self, notification: *mut Object) {
            self.did_finish_launching(notification);
        }

        #[unsafe(method(applicationShouldOpenUntitledFile:))]
        const fn _should_open_untitled_file(&self, _: *mut Object) -> Bool {
            Bool::YES
        }

        #[unsafe(method(applicationOpenUntitledFile:))]
        fn _open_untitled_file(&self, _: *mut Object) -> Bool {
            // SAFETY: AppKit calls this on the main thread. The shared document controller is
            // live for the application lifetime and handles the panel and opened documents.
            unsafe {
                let controller: *mut Object =
                    msg_send![class!(NSDocumentController), sharedDocumentController];
                let _: () = msg_send![controller, openDocument: null_mut::<Object>()];
            }
            Bool::YES
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        const fn _should_terminate_after_last_window(&self, _: *mut Object) -> Bool {
            Bool::NO
        }
    }
);

impl AppDelegate {
    fn did_finish_launching(&self, notification: *mut Object) {
        // SAFETY: AppKit supplies a valid notification containing NSApplication.
        unsafe {
            let application: *mut Object = msg_send![notification, object];
            let _: () = msg_send![application, activateIgnoringOtherApps: Bool::YES];
        }
    }
}

fn set_error(error_out: *mut c_void, description: &str) {
    if error_out.is_null() {
        return;
    }
    let domain = ns_string!("nl.bplaat.ImageView");
    // SAFETY: error_out is a non-null NSError** supplied by NSDocument. The error is
    // autoreleased and remains valid for the current event cycle.
    unsafe {
        error_out
            .cast::<*mut Object>()
            .write(make_error(domain, description));
    }
}

fn menu_item(
    title: &str,
    action: objc2::runtime::Sel,
    key: &str,
    modifiers: u64,
    target: *mut Object,
) -> Retained<Object> {
    // SAFETY: NSMenuItem's designated initializer accepts these NSStrings and selector.
    unsafe {
        let item: Allocated<Object> = msg_send![class!(NSMenuItem), alloc];
        let item: Retained<Object> = msg_send![item,
            initWithTitle: ns_string(title),
            action: action,
            keyEquivalent: ns_string(key)
        ];
        let _: () = msg_send![&*item, setKeyEquivalentModifierMask: modifiers];
        if !target.is_null() {
            let _: () = msg_send![&*item, setTarget: target];
        }
        item
    }
}

fn add_menu(main_menu: &Object, title: &str) -> Retained<Object> {
    // SAFETY: main_menu is a valid NSMenu. It retains the item, which retains the submenu.
    unsafe {
        let item: Retained<Object> = msg_send![class!(NSMenuItem), new];
        if !title.is_empty() {
            let _: () = msg_send![&*item, setTitle: ns_string(title)];
        }
        let menu: Allocated<Object> = msg_send![class!(NSMenu), alloc];
        let menu: Retained<Object> = msg_send![menu, initWithTitle: ns_string(title)];
        let _: () = msg_send![&*item, setSubmenu: menu.as_ptr()];
        let _: () = msg_send![main_menu, addItem: item.as_ptr()];
        menu
    }
}

fn add_item(
    menu: &Object,
    title: &str,
    action: objc2::runtime::Sel,
    key: &str,
    modifiers: u64,
    target: *mut Object,
) {
    // SAFETY: menu is a valid NSMenu and retains the item before its Rust owner is dropped.
    unsafe {
        let item = menu_item(title, action, key, modifiers, target);
        let _: () = msg_send![menu, addItem: item.as_ptr()];
    }
}

// Returns the key equivalent of a function key, which AppKit spells as a single character.
fn function_key(code: u16) -> String {
    char::from_u32(u32::from(code))
        .expect("function keys are characters of the private use area")
        .to_string()
}

// Adds the named recent-files submenu that `NSDocumentController` maintains when supported.
fn add_open_recent_menu(menu: &Object) {
    // SAFETY: menu is a valid NSMenu that retains the item, which retains the submenu.
    unsafe {
        let item: Retained<Object> = msg_send![class!(NSMenuItem), new];
        let _: () = msg_send![&*item, setTitle: ns_string!("Open Recent")];
        let recent_menu: Allocated<Object> = msg_send![class!(NSMenu), alloc];
        let recent_menu: Retained<Object> =
            msg_send![recent_menu, initWithTitle: ns_string!("Open Recent")];
        let named: Bool = msg_send![&*recent_menu, respondsToSelector: sel!(_setMenuName:)];
        if named.as_bool() {
            let _: () = msg_send![&*recent_menu, _setMenuName: ns_string!("NSRecentDocumentsMenu")];
        }
        add_item(
            &recent_menu,
            "Clear Menu",
            sel!(clearRecentDocuments:),
            "",
            0,
            null_mut(),
        );
        let _: () = msg_send![&*item, setSubmenu: recent_menu.as_ptr()];
        let _: () = msg_send![menu, addItem: item.as_ptr()];
    }
}

fn add_separator(menu: &Object) {
    // SAFETY: menu is a valid NSMenu and separatorItem returns a valid shared menu item.
    unsafe {
        let separator: *mut Object = msg_send![class!(NSMenuItem), separatorItem];
        let _: () = msg_send![menu, addItem: separator];
    }
}

fn create_menu(application: *mut Object) {
    // SAFETY: application is the shared NSApplication and all menu ownership transfers follow
    // AppKit's retain conventions.
    unsafe {
        let main_menu: Retained<Object> = msg_send![class!(NSMenu), new];

        let app_menu = add_menu(&main_menu, "");
        add_item(
            &app_menu,
            "About ImageView",
            sel!(orderFrontStandardAboutPanel:),
            "",
            0,
            application,
        );
        add_separator(&app_menu);
        let services_item: Retained<Object> = msg_send![class!(NSMenuItem), new];
        let _: () = msg_send![&*services_item, setTitle: ns_string!("Services")];
        let services_menu: Retained<Object> = msg_send![class!(NSMenu), new];
        let _: () = msg_send![&*services_item, setSubmenu: services_menu.as_ptr()];
        let _: () = msg_send![&*app_menu, addItem: services_item.as_ptr()];
        let _: () = msg_send![application, setServicesMenu: services_menu.as_ptr()];
        add_separator(&app_menu);
        add_item(
            &app_menu,
            "Hide ImageView",
            sel!(hide:),
            "h",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_item(
            &app_menu,
            "Hide Others",
            sel!(hideOtherApplications:),
            "h",
            NS_EVENT_MODIFIER_FLAG_COMMAND | NS_EVENT_MODIFIER_FLAG_OPTION,
            null_mut(),
        );
        add_item(
            &app_menu,
            "Show All",
            sel!(unhideAllApplications:),
            "",
            0,
            null_mut(),
        );
        add_separator(&app_menu);
        add_item(
            &app_menu,
            "Quit ImageView",
            sel!(terminate:),
            "q",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        let file_menu = add_menu(&main_menu, "File");
        add_item(
            &file_menu,
            "Open...",
            sel!(openDocument:),
            "o",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_open_recent_menu(&file_menu);
        add_separator(&file_menu);
        add_item(
            &file_menu,
            "Print...",
            sel!(printDocument:),
            "p",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_separator(&file_menu);
        add_item(
            &file_menu,
            "Close Window",
            sel!(performClose:),
            "w",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        let edit_menu = add_menu(&main_menu, "Edit");
        add_item(
            &edit_menu,
            "Undo",
            sel!(undo:),
            "z",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_item(
            &edit_menu,
            "Redo",
            sel!(redo:),
            "z",
            NS_EVENT_MODIFIER_FLAG_COMMAND | NS_EVENT_MODIFIER_FLAG_SHIFT,
            null_mut(),
        );
        add_separator(&edit_menu);
        add_item(
            &edit_menu,
            "Cut",
            sel!(cut:),
            "x",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_item(
            &edit_menu,
            "Copy",
            sel!(copy:),
            "c",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_item(
            &edit_menu,
            "Paste",
            sel!(paste:),
            "v",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_item(&edit_menu, "Delete", sel!(delete:), "", 0, null_mut());
        add_item(
            &edit_menu,
            "Select All",
            sel!(selectAll:),
            "a",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        let view_menu = add_menu(&main_menu, "View");
        add_item(
            &view_menu,
            "Zoom In",
            sel!(zoomIn:),
            "+",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_item(
            &view_menu,
            "Zoom Out",
            sel!(zoomOut:),
            "-",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_separator(&view_menu);
        add_item(
            &view_menu,
            "Actual Size",
            sel!(actualSize:),
            "0",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_item(
            &view_menu,
            "Zoom to Fit",
            sel!(zoomToFit:),
            "9",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_separator(&view_menu);
        add_item(
            &view_menu,
            "Previous Image",
            sel!(previousImage:),
            &function_key(NS_LEFT_ARROW_FUNCTION_KEY),
            0,
            null_mut(),
        );
        add_item(
            &view_menu,
            "Next Image",
            sel!(nextImage:),
            &function_key(NS_RIGHT_ARROW_FUNCTION_KEY),
            0,
            null_mut(),
        );
        add_separator(&view_menu);
        // AppKit renames the item to Exit Full Screen while a window is in full screen itself.
        add_item(
            &view_menu,
            "Enter Full Screen",
            sel!(toggleFullScreen:),
            "f",
            NS_EVENT_MODIFIER_FLAG_COMMAND | NS_EVENT_MODIFIER_FLAG_CONTROL,
            null_mut(),
        );
        let window_menu = add_menu(&main_menu, "Window");
        add_item(
            &window_menu,
            "Minimize",
            sel!(performMiniaturize:),
            "m",
            NS_EVENT_MODIFIER_FLAG_COMMAND,
            null_mut(),
        );
        add_item(&window_menu, "Zoom", sel!(performZoom:), "", 0, null_mut());
        let _: () = msg_send![application, setWindowsMenu: window_menu.as_ptr()];

        let help_menu = add_menu(&main_menu, "Help");
        let _: () = msg_send![application, setHelpMenu: help_menu.as_ptr()];

        let _: () = msg_send![application, setMainMenu: main_menu.as_ptr()];
    }
}

fn main() {
    // SAFETY: The shared application and registered delegate/document classes remain alive for
    // the entire AppKit run loop. Selectors use their documented Objective-C signatures.
    autoreleasepool(|_| unsafe {
        let _ = Document::class();
        enable_concurrent_document_reading();
        let application: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        // Every document has its own window, so disable window tabbing.
        let _: () = msg_send![class!(NSWindow), setAllowsAutomaticWindowTabbing: Bool::NO];
        let _: Bool = msg_send![application,
            setActivationPolicy: NS_APPLICATION_ACTIVATION_POLICY_REGULAR
        ];
        let delegate: Retained<Object> = msg_send![AppDelegate::class(), new];
        let _: () = msg_send![application, setDelegate: delegate.as_ptr()];
        create_menu(application);
        let _: () = msg_send![application, run];
    });
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn coordinated_load_reads_and_classifies_an_image() {
        autoreleasepool(|_| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rgb.qoi");
            // SAFETY: path names an existing image and the autorelease pool keeps its URL alive.
            let (media, kind, _) = unsafe {
                let url = file_url(&path);
                load_document(url).expect("example image should load")
            };
            assert!(media.pixel_size().is_some());
            // SAFETY: kind owns a live NSString for this scope.
            let identifier: *const std::ffi::c_char =
                unsafe { msg_send![kind.as_ptr(), UTF8String] };
            // SAFETY: NSString keeps its UTF-8 representation alive while kind is alive.
            let identifier = unsafe { std::ffi::CStr::from_ptr(identifier) };
            assert_eq!(identifier.to_bytes(), b"org.qoiformat.qoi");
        });
    }

    #[test]
    fn bmp_uses_its_decoded_pixel_dimensions() {
        autoreleasepool(|_| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/dice.bmp");
            // SAFETY: path names an existing image and the autorelease pool keeps its URL alive.
            let (media, _, _) = unsafe {
                let url = file_url(&path);
                load_document(url).expect("BMP example should load")
            };
            let image = media;
            assert_eq!((image.size().width, image.size().height), (800.0, 600.0));
            let size = image
                .pixel_size()
                .expect("BMP should have decoded dimensions");
            assert_eq!((size.width, size.height), (800.0, 600.0));
        });
    }

    #[test]
    fn dib_and_jfif_examples_load() {
        autoreleasepool(|_| {
            for extension in ["dib", "jfif"] {
                let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join(format!("examples/dice.{extension}"));
                // SAFETY: Each path names an existing image and the pool keeps its URL alive.
                let (media, _, _) = unsafe { load_document(file_url(&path)) }
                    .unwrap_or_else(|error| panic!("failed to load .{extension}: {error}"));
                let size = media.pixel_size().expect("example has decoded dimensions");
                assert_eq!((size.width, size.height), (800.0, 600.0));
            }
        });
    }

    #[test]
    fn common_raster_extensions_load() {
        autoreleasepool(|_| {
            let fixtures =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../imageview-appkit/tests/fixtures");
            let folder = std::env::temp_dir().join(format!(
                "imageview-raster-extensions-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&folder).expect("create raster extension test folder");
            for (extension, fixture) in [
                ("bmp", "rgb.bmp"),
                ("dib", "rgb.bmp"),
                ("jpg", "progressive.jpg"),
                ("jpeg", "progressive.jpg"),
                ("jpe", "progressive.jpg"),
                ("jfif", "progressive.jpg"),
                ("png", "rgb.png"),
                ("gif", "animated.gif"),
            ] {
                let path = folder.join(format!("image.{extension}"));
                let bytes = std::fs::read(fixtures.join(fixture)).expect("read raster fixture");
                let bytes = if extension == "dib" {
                    &bytes[14..]
                } else {
                    &bytes
                };
                std::fs::write(&path, bytes).expect("write raster fixture");
                // SAFETY: Each URL names an existing image and the pool keeps the URL alive.
                let result = unsafe { load_document(file_url(&path)) };
                result.unwrap_or_else(|error| panic!("failed to load .{extension}: {error}"));
            }
            std::fs::remove_dir_all(folder).expect("remove raster extension test folder");
        });
    }

    #[test]
    fn special_extensions_use_their_declared_image_types() {
        autoreleasepool(|_| {
            for (extension, expected) in [
                ("APNG", b"org.libpng.apng".as_slice()),
                ("BMP", b"com.microsoft.bmp".as_slice()),
                ("JFIF", b"org.jpeg.jfif".as_slice()),
                ("QOI", b"org.qoiformat.qoi".as_slice()),
                ("TVG", b"org.tinyvg.tvg".as_slice()),
            ] {
                // SAFETY: Type lookup only accesses immutable UTI and NSString objects.
                let identifier = unsafe { type_identifier(extension) }.expect("declared type");
                // SAFETY: identifier keeps its null-terminated UTF-8 representation alive.
                let actual = unsafe {
                    let bytes: *const std::ffi::c_char = msg_send![identifier.as_ptr(), UTF8String];
                    std::ffi::CStr::from_ptr(bytes).to_bytes()
                };
                assert_eq!(actual, expected);
            }
        });
    }

    #[test]
    fn document_convenience_initializer_initializes_ivars_once() {
        let kind = ns_string!("org.qoiformat.qoi");
        autoreleasepool(|_| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rgb.qoi");
            // SAFETY: The URL and type describe an existing image. NSDocument's inherited
            // convenience initializer calls Document's init, which initializes the Rust ivars.
            let document: Option<Retained<Document>> = unsafe {
                let url = file_url(&path);
                let document: Allocated<Document> = msg_send![Document::class(), alloc];
                msg_send![document,
                    initWithContentsOfURL: url,
                    ofType: kind,
                    error: null_mut::<c_void>()
                ]
            };
            assert!(document.is_some());
        });
    }

    #[test]
    fn rejects_empty_document_data_without_dereferencing_null_bytes() {
        autoreleasepool(|_| {
            // SAFETY: NSData's data constructor returns a live empty data object.
            let result = unsafe {
                let data: *mut Object = msg_send![class!(NSData), data];
                decode_document(data)
            };
            assert!(result.is_err());
        });
    }
}
