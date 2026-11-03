/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The WinUI 3 window: a XAML island with a header bar in the title bar, like a GTK header bar
//! or a unified macOS toolbar, above a scroll viewer that zooms and pans the media. The Windows
//! App Runtime is loaded when ImageView starts, an MSIX package depends on it in its manifest and
//! an unpackaged build adds it to the process with the dynamic dependency API, like a WebView2
//! app expects its runtime to be installed.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::OnceLock;

use imageview_core::{MAXIMUM_MAGNIFICATION, MINIMUM_MAGNIFICATION};

use super::headers::*;
use super::winrt::*;

/// The height of the header bar, the tall caption buttons of the window fill it.
pub(crate) const HEADER_HEIGHT: f64 = 48.0;

// The oldest Windows App SDK release ImageView supports, cargo-bundle writes the same
// dependency into the MSIX manifest.
const FRAMEWORK_FAMILY: &str = "Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe";
const MIN_VERSION: u64 = 2 << 48;
const MIN_VERSION_NAME: &str = "2.0";
const DOWNLOAD_URL: &str = "https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads";

const STATEREPOSITORY_E_DEPENDENCY_NOT_RESOLVED: HRESULT = 0x8067_0016_u32 as i32;

// The header bar buttons and the application actions they perform.
const ACTIONS: [(&str, &str); 5] = [
    ("Previous", "view.previous"),
    ("Next", "view.next"),
    ("ZoomOut", "view.zoom_out"),
    ("ResetZoom", "view.zoom_to_fit"),
    ("ZoomIn", "view.zoom_in"),
];

// The scroll viewer zooms from 0.1, so it shows the media at this scale and zooms that by the
// magnification divided by it, which keeps the smallest magnification in its range.
const MIN_ZOOM_FACTOR: f64 = 0.1;
const CONTENT_SCALE: f64 = MINIMUM_MAGNIFICATION / MIN_ZOOM_FACTOR;
const MAX_ZOOM_FACTOR: f64 = MAXIMUM_MAGNIFICATION / CONTENT_SCALE;

// Where the buttons start, after the app icon.
const BUTTONS_LEFT: f64 = 44.0;
// The space between the zoom buttons and the caption buttons.
const END_GAP: f64 = 8.0;

/// The narrowest header bar that fits the icon, the navigation buttons, a trimmed title, the zoom
/// buttons and the tall caption buttons, so they never overlap.
pub(crate) const MIN_WIDTH: f64 = BUTTONS_LEFT
    // Previous and next.
    + 2.0 * 40.0
    // A few characters of the title and its margins.
    + 120.0
    // The zoom out, reset and zoom in buttons and their border.
    + 36.0 + 56.0 + 36.0 + 2.0
    + END_GAP
    // The three tall caption buttons.
    + 3.0 * 48.0;

// The whole window: one header bar, like a GTK header bar or a unified macOS toolbar, with the
// app icon and navigation buttons, the centered title and the zoom buttons before the caption
// buttons, above a scroll viewer that zooms and pans the media over a checkerboard.
const XAML: &str = include_str!("winui.xaml");

/// Why WinUI could not start.
enum StartError {
    // The Windows App Runtime is not installed.
    Missing,
    // Windows has no dynamic dependency API, it was added in Windows 11.
    Unsupported,
    // Creating part of WinUI failed.
    Failed(&'static str, HRESULT),
}

impl StartError {
    fn show(&self, hwnd: HWND) {
        let architecture = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x64",
            architecture => architecture,
        };
        let (text, caption, kind) = match self {
            Self::Missing => (
                format!(
                    "ImageView needs the Windows App Runtime {MIN_VERSION_NAME} or newer \
                     ({architecture}).\n\nDownload it now?"
                ),
                "Windows App Runtime Required",
                MB_YESNO | MB_ICONWARNING,
            ),
            Self::Unsupported => (
                "ImageView needs Windows 11 to load the Windows App Runtime.".to_owned(),
                "ImageView",
                MB_OK | MB_ICONERROR,
            ),
            Self::Failed(component, result) => (
                format!(
                    "Failed to create the WinUI {component} (HRESULT 0x{:08X}).",
                    *result as u32
                ),
                "WinUI Error",
                MB_OK | MB_ICONERROR,
            ),
        };
        let (text, caption) = (wide(&text), wide(caption));
        // SAFETY: The strings outlive the message box.
        let answer = unsafe { MessageBoxW(hwnd, text.as_ptr(), caption.as_ptr(), kind) };
        if matches!(self, Self::Missing) && answer == IDYES {
            let (open, url) = (wide("open"), wide(DOWNLOAD_URL));
            // SAFETY: The strings outlive the call.
            unsafe {
                ShellExecuteW(
                    hwnd,
                    open.as_ptr(),
                    url.as_ptr(),
                    null(),
                    null(),
                    SW_SHOWNORMAL,
                )
            };
        }
    }
}

/// Loads the Windows App Runtime and starts XAML on this thread, or explains why it can not
/// and exits.
pub(crate) fn init() {
    if let Err(error) = start() {
        error.show(null_mut());
        std::process::exit(1);
    }
}

fn start() -> std::result::Result<(), StartError> {
    if !is_packaged() {
        add_framework()?;
    }
    let failed = |component| move |result| StartError::Failed(component, result);
    // XAML runs on the dispatcher queue of this thread, and keeps running while windows come
    // and go when the thread keeps its XAML manager. Both live as long as the process.
    // SAFETY: Slot 7 of the statics is CreateOnCurrentThread.
    let dispatcher = factory(
        "Microsoft.UI.Dispatching.DispatcherQueueController",
        &IID_IDISPATCHER_QUEUE_CONTROLLER_STATICS,
    )
    .and_then(|statics| unsafe { statics.get(7) })
    .map_err(failed("dispatcher queue"))?;
    // The application comes first, otherwise XAML starts with a default one.
    let application = create_application().map_err(failed("application"))?;
    // SAFETY: Slot 6 of the statics is InitializeForCurrentThread.
    let manager = factory(
        "Microsoft.UI.Xaml.Hosting.WindowsXamlManager",
        &IID_IWINDOWS_XAML_MANAGER_STATICS,
    )
    .and_then(|statics| unsafe { statics.get(6) })
    .map_err(failed("XAML manager"))?;
    // The Fluent styles of the WinUI controls, like the App.xaml of a WinUI app has.
    let resources = activate("Microsoft.UI.Xaml.Controls.XamlControlsResources")
        .and_then(|resources| resources.cast(&IID_IRESOURCE_DICTIONARY))
        .map_err(failed("controls resources"))?;
    // SAFETY: Slot 7 of IApplication is put_Resources.
    unsafe { application.put(7, resources.as_raw()) }.map_err(failed("controls resources"))?;
    std::mem::forget((dispatcher, manager, application));
    Ok(())
}

// A packaged app has the framework in its package graph from its manifest.
fn is_packaged() -> bool {
    let mut length = 0;
    // SAFETY: Asking for the length only, which fails without a package.
    unsafe { GetCurrentPackageFullName(&mut length, null_mut()) == ERROR_INSUFFICIENT_BUFFER }
}

// Adds the Windows App Runtime framework package to the package graph of this process.
fn add_framework() -> std::result::Result<(), StartError> {
    type TryCreatePackageDependency = unsafe extern "system" fn(
        user: *mut c_void,
        family: *const u16,
        min_version: u64,
        architectures: u32,
        lifetime_kind: i32,
        lifetime_artifact: *const u16,
        options: i32,
        id: *mut *mut u16,
    ) -> HRESULT;
    type AddPackageDependency = unsafe extern "system" fn(
        id: *const u16,
        rank: i32,
        options: i32,
        context: *mut *mut c_void,
        full_name: *mut *mut u16,
    ) -> HRESULT;

    // SAFETY: kernelbase.dll is always loaded and the functions have these signatures.
    let (try_create, add) = unsafe {
        let kernelbase = GetModuleHandleW(wide("kernelbase.dll").as_ptr());
        let try_create = GetProcAddress(kernelbase, c"TryCreatePackageDependency".as_ptr());
        let add = GetProcAddress(kernelbase, c"AddPackageDependency".as_ptr());
        if try_create.is_null() || add.is_null() {
            return Err(StartError::Unsupported);
        }
        (
            std::mem::transmute::<*mut c_void, TryCreatePackageDependency>(try_create),
            std::mem::transmute::<*mut c_void, AddPackageDependency>(add),
        )
    };
    let architecture = match std::env::consts::ARCH {
        "x86" => 0x2,
        "x86_64" => 0x4,
        "aarch64" => 0x10,
        _ => 0,
    };
    let not_resolved = |result| {
        if result == STATEREPOSITORY_E_DEPENDENCY_NOT_RESOLVED {
            StartError::Missing
        } else {
            StartError::Failed("runtime dependency", result)
        }
    };
    let family = wide(FRAMEWORK_FAMILY);
    let mut id = null_mut();
    // SAFETY: The dependency lives as long as the process, its strings are freed from the
    // process heap.
    unsafe {
        // The neutral architecture flag also accepts the architecture neutral parts.
        check(try_create(
            null_mut(),
            family.as_ptr(),
            MIN_VERSION,
            architecture | 0x1,
            0,
            null(),
            0,
            &mut id,
        ))
        .map_err(not_resolved)?;
        let mut context = null_mut();
        let mut full_name = null_mut();
        let result = add(id, 0, 0, &mut context, &mut full_name);
        HeapFree(GetProcessHeap(), 0, id.cast());
        HeapFree(GetProcessHeap(), 0, full_name.cast());
        check(result).map_err(not_resolved)
    }
}

/// Shows an error about WinUI and exits.
fn fail(hwnd: HWND, component: &'static str, result: HRESULT) -> ! {
    StartError::Failed(component, result).show(hwnd);
    std::process::exit(1);
}

// Returns the `Microsoft.UI.WindowId` of a window.
fn window_id(hwnd: HWND) -> Result<WindowId> {
    type GetWindowIdFromWindow = unsafe extern "system" fn(HWND, *mut WindowId) -> HRESULT;
    // The framework library comes from the package graph and stays loaded.
    static FUNCTION: OnceLock<usize> = OnceLock::new();
    let function = *FUNCTION.get_or_init(|| {
        // SAFETY: The names are valid null-terminated strings.
        unsafe {
            let library = LoadLibraryW(wide("Microsoft.Internal.FrameworkUdk.dll").as_ptr());
            GetProcAddress(library, c"Windowing_GetWindowIdFromWindow".as_ptr()) as usize
        }
    });
    if function == 0 {
        return Err(0x8000_4005_u32 as i32);
    }
    // SAFETY: The library exports this function with this signature.
    unsafe {
        let function = std::mem::transmute::<usize, GetWindowIdFromWindow>(function);
        let mut id = WindowId { value: 0 };
        check(function(hwnd, &mut id))?;
        Ok(id)
    }
}

/// A XAML bitmap of premultiplied BGRA pixels.
pub(crate) struct Bitmap {
    bitmap: ComPtr,
    source: ComPtr,
    size: (u32, u32),
}

impl Bitmap {
    /// Creates a transparent bitmap.
    pub(crate) fn new(width: u32, height: u32) -> Option<Self> {
        let factory = factory(
            "Microsoft.UI.Xaml.Media.Imaging.WriteableBitmap",
            &IID_IWRITEABLE_BITMAP_FACTORY,
        )
        .ok()?;
        let (width, height) = (i32::try_from(width).ok()?, i32::try_from(height).ok()?);
        // SAFETY: Slot 6 of the factory is CreateInstanceWithDimensions.
        let bitmap = out(|raw| unsafe {
            call!(factory, 6, width => i32, height => i32, raw => *mut *mut c_void)
        })
        .ok()?;
        Some(Self {
            source: bitmap.cast(&IID_IIMAGE_SOURCE).ok()?,
            bitmap: bitmap.cast(&IID_IWRITEABLE_BITMAP).ok()?,
            size: (width as u32, height as u32),
        })
    }

    /// The width and height in pixels.
    pub(crate) const fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Replaces the pixels, which are as many as the bitmap has.
    pub(crate) fn write(&self, pixels: &[u8]) {
        let length = self.size.0 as usize * self.size.1 as usize * 4;
        assert_eq!(pixels.len(), length, "bitmap pixels have the wrong size");
        // SAFETY: Slot 6 of IWriteableBitmap is get_PixelBuffer, slot 3 of IBufferByteAccess is
        // Buffer, which holds the pixels of the bitmap, and slot 7 of IWriteableBitmap is
        // Invalidate.
        unsafe {
            let Ok(buffer) = self
                .bitmap
                .get(6)
                .and_then(|buffer| buffer.cast(&IID_IBUFFER_BYTE_ACCESS))
            else {
                return;
            };
            let mut bytes = null_mut::<u8>();
            if check(call!(buffer, 3, &mut bytes => *mut *mut u8)).is_ok() && !bytes.is_null() {
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), bytes, length);
                call!(self.bitmap, 7);
            }
        }
    }
}

// The icon of the executable as a bitmap of `size` pixels.
fn app_icon(size: i32) -> Option<Bitmap> {
    let pixels = icon_pixels(size)?;
    let bitmap = Bitmap::new(size as u32, size as u32)?;
    bitmap.write(&pixels);
    Some(bitmap)
}

// The premultiplied BGRA pixels of the executable icon, which the resource compiler stores as
// resource 1, at `size` pixels.
fn icon_pixels(size: i32) -> Option<Vec<u8>> {
    // SAFETY: The icon and its bitmaps are freed after their pixels are copied.
    unsafe {
        let icon = LoadImageW(
            GetModuleHandleW(null()),
            std::ptr::without_provenance(1),
            IMAGE_ICON,
            size,
            size,
            0,
        );
        if icon.is_null() {
            return None;
        }
        let mut info = std::mem::zeroed::<ICONINFO>();
        let found = GetIconInfo(icon, &mut info) != 0;
        DestroyIcon(icon);
        if !found {
            return None;
        }
        let mut header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        let hdc = GetDC(null_mut());
        let lines = GetDIBits(
            hdc,
            info.hbmColor,
            0,
            size as u32,
            pixels.as_mut_ptr().cast(),
            &mut header,
            DIB_RGB_COLORS,
        );
        ReleaseDC(null_mut(), hdc);
        DeleteObject(info.hbmColor);
        DeleteObject(info.hbmMask);
        if lines != size {
            return None;
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = (u16::from(*channel) * alpha / 255) as u8;
            }
        }
        Some(pixels)
    }
}

// Boxes a value for a WinRT parameter that takes an `IReference<T>`, slot 14 of the property
// value statics is CreateSingle and slot 15 is CreateDouble.
fn boxed<T: Copy>(slot: usize, value: T, iid: &GUID) -> Option<ComPtr> {
    let statics = factory(
        "Windows.Foundation.PropertyValue",
        &IID_IPROPERTY_VALUE_STATICS,
    )
    .ok()?;
    // SAFETY: The caller passes the slot creating a property value of this type.
    out(|raw| unsafe { call!(statics, slot, value => T, raw => *mut *mut c_void) })
        .and_then(|value| value.cast(iid))
        .ok()
}

/// The XAML island filling a window: the header bar in its extended title bar next to the
/// caption buttons, and the scroll viewer that zooms and pans the media below it.
pub(crate) struct Island {
    source: ComPtr,
    bridge: ComPtr,
    root: ComPtr,
    title: ComPtr,
    subtitle: ComPtr,
    heading: ComPtr,
    title_bar: ComPtr,
    scroller: ComPtr,
    media: ComPtr,
    checkerboard: ComPtr,
    regions: Rc<Regions>,
    // A view change asked for before the scroll viewer was laid out, which it would ignore.
    pending: Rc<Cell<Option<View>>>,
}

// The button groups in the header bar, which take clicks instead of dragging the window.
struct Regions {
    non_client: ComPtr,
    title_bar: ComPtr,
    start: ComPtr,
    end: ComPtr,
    // The width and header bar height in physical pixels and the scale.
    size: Cell<(i32, i32, f64)>,
}

impl Regions {
    fn update(&self) {
        let (width, height, scale) = self.size.get();
        if width == 0 {
            return;
        }
        let mut inset = 0;
        // SAFETY: Slot 36 of IAppWindowTitleBar is get_RightInset and slot 32 of
        // IFrameworkElement is put_Margin.
        unsafe {
            call!(self.title_bar, 36, &mut inset => *mut i32);
            let margin = Thickness {
                left: 0.0,
                top: 0.0,
                right: f64::from(inset) / scale + END_GAP,
                bottom: 0.0,
            };
            call!(self.end, 32, margin => Thickness);
        }
        let actual_width = |element: &ComPtr| {
            let mut width = 0.0;
            // SAFETY: Slot 13 of IFrameworkElement is get_ActualWidth.
            unsafe { call!(element, 13, &mut width => *mut f64) };
            (width * scale).round() as i32
        };
        let (start, end) = (actual_width(&self.start), actual_width(&self.end));
        let rects = [
            RectInt32 {
                x: (BUTTONS_LEFT * scale).round() as i32,
                y: 0,
                width: start,
                height,
            },
            RectInt32 {
                x: width - inset - (END_GAP * scale).round() as i32 - end,
                y: 0,
                width: end,
                height,
            },
        ];
        // SAFETY: Slot 10 of IInputNonClientPointerSource is SetRegionRects with a region kind,
        // 9 is passthrough, and an array of rectangles.
        unsafe {
            call!(
                self.non_client,
                10,
                9i32 => i32,
                rects.len() as u32 => u32,
                rects.as_ptr() => *const RectInt32,
            )
        };
    }
}

// Where a drag with the primary button started: the pointer and the scroll offsets.
type Drag = (Point, (f64, f64));

// Scroll offsets and a magnification, keeping what is not given.
type View = (Option<f64>, Option<f64>, Option<f64>);

impl Island {
    /// Creates the island in `parent`, which fills the client area of `window`. The buttons call
    /// `on_action` with their application action, and `on_zoomed` follows the end of every
    /// change of the zoom or scroll position.
    pub(crate) fn new(
        parent: HWND,
        window: HWND,
        dark: bool,
        on_action: impl Fn(&'static str) + 'static,
        on_zoomed: impl Fn() + 'static,
    ) -> Self {
        Self::create(parent, window, on_action, on_zoomed)
            .inspect(|island| island.set_dark(dark))
            .unwrap_or_else(|(component, result)| fail(window, component, result))
    }

    fn create(
        parent: HWND,
        window: HWND,
        on_action: impl Fn(&'static str) + 'static,
        on_zoomed: impl Fn() + 'static,
    ) -> std::result::Result<Self, (&'static str, HRESULT)> {
        let failed = |component| move |result| (component, result);

        // The content extends into the title bar, under the caption buttons.
        let title_bar = factory("Microsoft.UI.Windowing.AppWindow", &IID_IAPP_WINDOW_STATICS)
            .and_then(|statics| {
                let id = window_id(window)?;
                // SAFETY: Slot 9 of the statics is GetFromWindowId.
                let app_window = out(|raw| unsafe {
                    call!(statics, 9, id => WindowId, raw => *mut *mut c_void)
                })?;
                // SAFETY: Slot 16 of IAppWindow is get_TitleBar.
                unsafe { app_window.get(16) }
            })
            .map_err(failed("title bar"))?;
        // SAFETY: Slot 25 is put_ExtendsContentIntoTitleBar, slot 7 of IAppWindowTitleBar2 is
        // put_PreferredHeightOption with 1 for tall caption buttons.
        unsafe {
            check(call!(title_bar, 25, 1u8 => u8)).map_err(failed("title bar"))?;
            let title_bar2 = title_bar
                .cast(&IID_IAPP_WINDOW_TITLE_BAR2)
                .map_err(failed("title bar"))?;
            check(call!(title_bar2, 7, 1i32 => i32)).map_err(failed("title bar"))?;
        }

        // SAFETY: Slot 17 is Initialize, slot 11 is get_SiteBridge and slot 17 of
        // IDesktopSiteBridge is Show.
        let (source, bridge) = unsafe {
            let source = compose(
                "Microsoft.UI.Xaml.Hosting.DesktopWindowXamlSource",
                &IID_IDESKTOP_WINDOW_XAML_SOURCE_FACTORY,
            )
            .map_err(failed("XAML island"))?;
            let id = window_id(parent).map_err(failed("XAML island"))?;
            check(call!(source, 17, id => WindowId)).map_err(failed("XAML island"))?;
            let bridge = source
                .get(11)
                .and_then(|bridge| bridge.cast(&IID_IDESKTOP_SITE_BRIDGE))
                .map_err(failed("XAML island"))?;
            check(call!(bridge, 17)).map_err(failed("XAML island"))?;
            (source, bridge)
        };

        let xaml = XAML
            .replace("{MIN_ZOOM}", &MIN_ZOOM_FACTOR.to_string())
            .replace("{MAX_ZOOM}", &MAX_ZOOM_FACTOR.to_string());
        let root = factory(
            "Microsoft.UI.Xaml.Markup.XamlReader",
            &IID_IXAML_READER_STATICS,
        )
        .and_then(|statics| {
            let xaml = HString::new(&xaml);
            // SAFETY: Slot 6 of the statics is Load.
            out(|raw| unsafe {
                call!(statics, 6, xaml.as_raw() => HSTRING, raw => *mut *mut c_void)
            })
        })
        .and_then(|root| root.cast(&IID_IFRAMEWORK_ELEMENT))
        .map_err(failed("window content"))?;
        let element = root
            .cast(&IID_IUI_ELEMENT)
            .map_err(failed("window content"))?;
        // SAFETY: Slot 7 of IDesktopWindowXamlSource is put_Content.
        unsafe { source.put(7, element.as_raw()) }.map_err(failed("window content"))?;

        let find = |name: &str, iid: &GUID| {
            let name = HString::new(name);
            // SAFETY: Slot 77 of IFrameworkElement is FindName.
            out(|raw| unsafe { call!(root, 77, name.as_raw() => HSTRING, raw => *mut *mut c_void) })
                .and_then(|element| element.cast(iid))
        };

        // Mica shows through the header bar where Windows supports it, otherwise it gets a solid
        // background. The backdrop needs the content of the island to attach to.
        // SAFETY: Slot 10 is put_SystemBackdrop.
        let mica = compose(
            "Microsoft.UI.Xaml.Media.MicaBackdrop",
            &IID_IMICA_BACKDROP_FACTORY,
        )
        .and_then(|mica| mica.cast(&IID_ISYSTEM_BACKDROP))
        .and_then(|mica| unsafe { source.put(10, mica.as_raw()) });
        if mica.is_err() {
            let background =
                find("Background", &IID_IUI_ELEMENT).map_err(failed("window content"))?;
            // SAFETY: Slot 24 of IUIElement is put_Visibility, 0 is visible.
            unsafe { call!(background, 24, 0i32 => i32) };
        }
        let title = find("Title", &IID_ITEXT_BLOCK).map_err(failed("window content"))?;
        let subtitle = find("Subtitle", &IID_ITEXT_BLOCK).map_err(failed("window content"))?;
        let heading = find("Heading", &IID_IUI_ELEMENT).map_err(failed("window content"))?;
        // SAFETY: The window is alive.
        let scale = f64::from(unsafe { GetDpiForWindow(window) }.max(96)) / 96.0;
        if let Some(icon) = app_icon((16.0 * scale).round() as i32) {
            let image = find("Icon", &IID_IIMAGE).map_err(failed("window content"))?;
            // SAFETY: Slot 7 of IImage is put_Source.
            _ = unsafe { image.put(7, icon.source.as_raw()) };
        }
        let zoom = find("Zoom", &IID_ITEXT_BLOCK).map_err(failed("window content"))?;
        let start = find("Start", &IID_IFRAMEWORK_ELEMENT).map_err(failed("window content"))?;
        let end = find("End", &IID_IFRAMEWORK_ELEMENT).map_err(failed("window content"))?;
        let scroller = find("Scroller", &IID_ISCROLL_VIEWER).map_err(failed("window content"))?;
        let media = find("Media", &IID_IIMAGE).map_err(failed("window content"))?;
        let checkerboard = find("Checkerboard", &IID_IIMAGE).map_err(failed("window content"))?;

        // The buttons in the title bar take clicks, the rest of it drags the window.
        let non_client = factory(
            "Microsoft.UI.Input.InputNonClientPointerSource",
            &IID_IINPUT_NON_CLIENT_POINTER_SOURCE_STATICS,
        )
        .and_then(|statics| {
            let id = window_id(window)?;
            // SAFETY: Slot 6 of the statics is GetForWindowId.
            out(|raw| unsafe { call!(statics, 6, id => WindowId, raw => *mut *mut c_void) })
        })
        .map_err(failed("title bar"))?;
        let regions = Rc::new(Regions {
            non_client,
            title_bar: title_bar.clone(),
            start: start.clone(),
            end: end.clone(),
            size: Cell::new((0, 0, 1.0)),
        });
        // The groups are measured after the island first shows them.
        let resized = Rc::downgrade(&regions);
        let resized = event_handler(IID_SIZE_CHANGED_EVENT_HANDLER, move |_, _| {
            if let Some(regions) = resized.upgrade() {
                regions.update();
            }
        });
        for group in [&start, &end] {
            let mut token = 0i64;
            // SAFETY: Slot 67 of IFrameworkElement is add_SizeChanged, the group keeps the
            // handler.
            check(unsafe {
                call!(group, 67, resized.as_raw() => *mut c_void, &mut token => *mut i64)
            })
            .map_err(failed("title bar"))?;
        }

        let clicked = event_handler(IID_ROUTED_EVENT_HANDLER, move |sender, _| {
            let Ok(sender) = sender.cast(&IID_IFRAMEWORK_ELEMENT) else {
                return;
            };
            let mut name = null_mut();
            // SAFETY: Slot 33 of IFrameworkElement is get_Name, which returns an owned string.
            if check(unsafe { call!(sender, 33, &mut name => *mut HSTRING) }).is_err() {
                return;
            }
            // SAFETY: The name is owned by this call.
            let name = unsafe { HString::from_raw(name) }.to_string_lossy();
            if let Some((_, action)) = ACTIONS.iter().find(|(button, _)| *button == name) {
                on_action(action);
            }
        });
        for (name, _) in ACTIONS {
            let button = find(name, &IID_IBUTTON_BASE).map_err(failed("window content"))?;
            let mut token = 0i64;
            // SAFETY: Slot 14 of IButtonBase is add_Click, the button keeps the handler.
            check(unsafe {
                call!(button, 14, clicked.as_raw() => *mut c_void, &mut token => *mut i64)
            })
            .map_err(failed("window content"))?;
        }

        // The zoom label follows every change, the application the final ones.
        // The handlers on the scroll viewer get it as their sender, so they keep no reference to
        // it that would keep it alive.
        let view_changed = event_handler(IID_VIEW_CHANGED_EVENT_HANDLER, move |sender, args| {
            let Ok(scroller) = sender.cast(&IID_ISCROLL_VIEWER) else {
                return;
            };
            let mut factor = 1.0f32;
            // SAFETY: Slot 54 of IScrollViewer is get_ZoomFactor.
            unsafe { call!(scroller, 54, &mut factor => *mut f32) };
            let magnification = f64::from(factor) * CONTENT_SCALE;
            let label = HString::new(&format!("{:.0}%", magnification * 100.0));
            // SAFETY: Slot 27 of ITextBlock is put_Text.
            _ = unsafe { zoom.put(27, label.as_raw()) };
            let mut intermediate = 0u8;
            if let Some(args) =
                args.and_then(|args| args.cast(&IID_ISCROLL_VIEWER_VIEW_CHANGED_EVENT_ARGS).ok())
            {
                // SAFETY: Slot 6 of the arguments is get_IsIntermediate.
                unsafe { call!(args, 6, &mut intermediate => *mut u8) };
            }
            if intermediate == 0 {
                on_zoomed();
            }
        });
        let mut token = 0i64;
        // SAFETY: Slot 74 of IScrollViewer is add_ViewChanged, the scroll viewer keeps the
        // handler.
        check(unsafe {
            call!(scroller, 74, view_changed.as_raw() => *mut c_void, &mut token => *mut i64)
        })
        .map_err(failed("window content"))?;

        Self::add_drag(&scroller).map_err(failed("window content"))?;

        // The view change from before the first layout applies once the scroll viewer has a size.
        let pending = Rc::new(Cell::<Option<View>>::new(None));
        let laid_out = {
            let pending = pending.clone();
            event_handler(IID_SIZE_CHANGED_EVENT_HANDLER, move |sender, _| {
                if let Ok(scroller) = sender.cast(&IID_ISCROLL_VIEWER)
                    && let Some((x, y, zoom)) = pending.take()
                {
                    change_view(&scroller, x, y, zoom);
                }
            })
        };
        let element = scroller
            .cast(&IID_IFRAMEWORK_ELEMENT)
            .map_err(failed("window content"))?;
        let mut token = 0i64;
        // SAFETY: Slot 67 of IFrameworkElement is add_SizeChanged, the scroll viewer keeps the
        // handler.
        check(unsafe {
            call!(element, 67, laid_out.as_raw() => *mut c_void, &mut token => *mut i64)
        })
        .map_err(failed("window content"))?;

        Ok(Self {
            source,
            bridge,
            root,
            title,
            subtitle,
            heading,
            title_bar,
            scroller,
            media,
            checkerboard,
            regions,
            pending,
        })
    }

    // Dragging with the primary mouse button pans the media, like the hand tool of other
    // viewers. Touch and touchpads pan with the scroll viewer itself. The handlers get the
    // scroll viewer as their sender, so they keep no reference to it that would keep it alive.
    fn add_drag(scroller: &ComPtr) -> Result<()> {
        let drag = Rc::new(Cell::<Option<Drag>>::new(None));
        let pressed = {
            let drag = drag.clone();
            event_handler(IID_POINTER_EVENT_HANDLER, move |sender, args| {
                let (Some((point, args)), Ok(scroller), Ok(element)) = (
                    args.and_then(|args| mouse_drag_point(sender, args)),
                    sender.cast(&IID_ISCROLL_VIEWER),
                    sender.cast(&IID_IUI_ELEMENT),
                ) else {
                    return;
                };
                drag.set(Some((point, scroll_offsets(&scroller))));
                // SAFETY: Slot 6 of IPointerRoutedEventArgs is get_Pointer, slot 212 of
                // IUIElement is CapturePointer and slot 9 of the arguments is put_Handled.
                unsafe {
                    if let Ok(pointer) = args.get(6) {
                        let mut captured = 0u8;
                        call!(element, 212, pointer.as_raw() => *mut c_void, &mut captured => *mut u8);
                    }
                    call!(args, 9, 1u8 => u8);
                }
            })
        };
        let moved = {
            let drag = drag.clone();
            event_handler(IID_POINTER_EVENT_HANDLER, move |sender, args| {
                let (Some((start, (x, y))), Some((point, _)), Ok(scroller)) = (
                    drag.get(),
                    args.and_then(|args| mouse_drag_point(sender, args)),
                    sender.cast(&IID_ISCROLL_VIEWER),
                ) else {
                    return;
                };
                change_view(
                    &scroller,
                    Some(x - f64::from(point.x - start.x)),
                    Some(y - f64::from(point.y - start.y)),
                    None,
                );
            })
        };
        let released = event_handler(IID_POINTER_EVENT_HANDLER, move |_, _| drag.set(None));
        let element = scroller.cast(&IID_IUI_ELEMENT)?;
        // Slots 152, 154, 156 and 162 of IUIElement are add_PointerPressed, add_PointerMoved,
        // add_PointerReleased and add_PointerCaptureLost.
        for (slot, handler) in [
            (152, &pressed),
            (154, &moved),
            (156, &released),
            (162, &released),
        ] {
            let mut token = 0i64;
            // SAFETY: These slots add a pointer event handler, the element keeps it.
            check(unsafe {
                call!(element, slot, handler.as_raw() => *mut c_void, &mut token => *mut i64)
            })?;
        }
        Ok(())
    }

    /// Places the island in its parent, in physical pixels.
    pub(crate) fn move_and_resize(&self, width: i32, height: i32, scale: f64) {
        let rect = RectInt32 {
            x: 0,
            y: 0,
            width,
            height,
        };
        // SAFETY: Slot 13 of IDesktopSiteBridge is MoveAndResize.
        unsafe { call!(self.bridge, 13, rect => RectInt32) };
        let header = ((HEADER_HEIGHT * scale).round() as i32).min(height);
        self.regions.size.set((width, header, scale));
        self.regions.update();
    }

    /// Shows the title and subtitle in the header bar.
    pub(crate) fn set_title(&self, title: &str, subtitle: &str) {
        for (text_block, text) in [(&self.title, title), (&self.subtitle, subtitle)] {
            let text = HString::new(text);
            // SAFETY: Slot 27 of ITextBlock is put_Text.
            _ = unsafe { text_block.put(27, text.as_raw()) };
        }
    }

    /// Dims the title while the window is inactive, like the system title bar does.
    pub(crate) fn set_active(&self, active: bool) {
        // SAFETY: Slot 10 of IUIElement is put_Opacity.
        unsafe { call!(self.heading, 10, if active { 1.0 } else { 0.5 } => f64) };
    }

    /// Shows a bitmap as the media, stretched to its size in logical pixels.
    pub(crate) fn set_media(&self, bitmap: &Bitmap, size: (f64, f64)) {
        set_image(
            &self.media,
            bitmap,
            (size.0 * CONTENT_SCALE, size.1 * CONTENT_SCALE),
        );
    }

    /// Shows a bitmap as the checkerboard behind the media, centered at its size in logical
    /// pixels.
    pub(crate) fn set_checkerboard(&self, bitmap: &Bitmap, size: (f64, f64)) {
        set_image(&self.checkerboard, bitmap, size);
    }

    /// The magnification of the media.
    pub(crate) fn zoom_factor(&self) -> f64 {
        let mut factor = 1.0f32;
        // SAFETY: Slot 54 of IScrollViewer is get_ZoomFactor.
        unsafe { call!(self.scroller, 54, &mut factor => *mut f32) };
        f64::from(factor) * CONTENT_SCALE
    }

    /// The scroll offsets of the magnified media, in logical pixels.
    pub(crate) fn offsets(&self) -> (f64, f64) {
        scroll_offsets(&self.scroller)
    }

    /// Scrolls and zooms right away, keeping what is not given, or once the scroll viewer is laid
    /// out.
    pub(crate) fn change_view(&self, x: Option<f64>, y: Option<f64>, zoom: Option<f64>) {
        let mut width = 0.0f64;
        // SAFETY: Slot 41 of IScrollViewer is get_ViewportWidth.
        unsafe { call!(self.scroller, 41, &mut width => *mut f64) };
        if width > 0.0 {
            change_view(&self.scroller, x, y, zoom);
        } else {
            self.pending.set(Some((x, y, zoom)));
        }
    }

    /// Gives the scroll viewer the keyboard focus, so the keys scroll the media.
    pub(crate) fn focus(&self) {
        if let Ok(element) = self.scroller.cast(&IID_IUI_ELEMENT) {
            let mut focused = 0u8;
            // SAFETY: Slot 226 of IUIElement is Focus, 3 is programmatic focus.
            unsafe { call!(element, 226, 3i32 => i32, &mut focused => *mut u8) };
        }
    }

    /// Follows the light or dark theme, also with the caption buttons.
    pub(crate) fn set_dark(&self, dark: bool) {
        // SAFETY: Slot 58 of IFrameworkElement is put_RequestedTheme, 1 is light and 2 is dark.
        unsafe { call!(self.root, 58, if dark { 2 } else { 1 } => i32) };

        let color = |a, value| Color {
            a,
            r: value,
            g: value,
            b: value,
        };
        // The Fluent caption button colors on a transparent background.
        let (foreground, hover, pressed, inactive) = if dark {
            (
                color(0xff, 0xff),
                color(0x0f, 0xff),
                color(0x0a, 0xff),
                color(0x5d, 0xff),
            )
        } else {
            (
                color(0xe4, 0x00),
                color(0x09, 0x00),
                color(0x06, 0x00),
                color(0x5c, 0x00),
            )
        };
        let transparent = color(0, 0);
        // The title bar the window draws behind the header bar shows while it catches up with a
        // resize, so it gets the background color of the theme instead of white.
        let background = color(0xff, if dark { 0x20 } else { 0xf3 });
        // The IAppWindowTitleBar setter slots of the title bar and caption button colors.
        for (slot, color) in [
            (7, background),
            (32, background),
            (9, transparent),
            (11, foreground),
            (13, hover),
            (15, foreground),
            (17, transparent),
            (19, inactive),
            (21, pressed),
            (23, foreground),
        ] {
            // SAFETY: These slots take an IReference<Color>.
            _ = unsafe { self.title_bar.put(slot, color_reference(color).as_raw()) };
        }
    }
}

impl Drop for Island {
    fn drop(&mut self) {
        if let Ok(closable) = self.source.cast(&IID_ICLOSABLE) {
            // SAFETY: Slot 6 of IClosable is Close.
            unsafe { call!(closable, 6) };
        }
    }
}

// Shows a bitmap in an image element at a size in logical pixels.
fn set_image(image: &ComPtr, bitmap: &Bitmap, (width, height): (f64, f64)) {
    // SAFETY: Slot 7 of IImage is put_Source, slots 16 and 18 of IFrameworkElement are
    // put_Width and put_Height.
    unsafe {
        _ = image.put(7, bitmap.source.as_raw());
        if let Ok(element) = image.cast(&IID_IFRAMEWORK_ELEMENT) {
            call!(element, 16, width => f64);
            call!(element, 18, height => f64);
        }
    }
}

// Scrolls and zooms a scroll viewer right away, keeping what is not given.
fn change_view(scroller: &ComPtr, x: Option<f64>, y: Option<f64>, zoom: Option<f64>) {
    let x = x.and_then(|x| boxed(15, x.max(0.0), &IID_IREFERENCE_DOUBLE));
    let y = y.and_then(|y| boxed(15, y.max(0.0), &IID_IREFERENCE_DOUBLE));
    let zoom = zoom.and_then(|magnification| {
        let factor = (magnification / CONTENT_SCALE).clamp(MIN_ZOOM_FACTOR, MAX_ZOOM_FACTOR);
        boxed(14, factor as f32, &IID_IREFERENCE_SINGLE)
    });
    let raw = |value: &Option<ComPtr>| value.as_ref().map_or(null_mut(), ComPtr::as_raw);
    let mut changed = 0u8;
    // SAFETY: Slot 84 of IScrollViewer is ChangeView with optional horizontal and vertical
    // offsets, an optional zoom factor and whether to skip the animation.
    unsafe {
        call!(
            scroller,
            84,
            raw(&x) => *mut c_void,
            raw(&y) => *mut c_void,
            raw(&zoom) => *mut c_void,
            1u8 => u8,
            &mut changed => *mut u8,
        )
    };
}

// The scroll offsets of a scroll viewer in logical pixels of the magnified media.
fn scroll_offsets(scroller: &ComPtr) -> (f64, f64) {
    let (mut x, mut y) = (0.0f64, 0.0f64);
    // SAFETY: Slots 40 and 45 of IScrollViewer are get_HorizontalOffset and get_VerticalOffset.
    unsafe {
        call!(scroller, 40, &mut x => *mut f64);
        call!(scroller, 45, &mut y => *mut f64);
    }
    (x, y)
}

// The position of a mouse pointer with the primary button down relative to the element that got
// the event, and the event arguments.
fn mouse_drag_point(element: &ComPtr, args: &ComPtr) -> Option<(Point, ComPtr)> {
    let args = args.cast(&IID_IPOINTER_ROUTED_EVENT_ARGS).ok()?;
    let element = element.cast(&IID_IUI_ELEMENT).ok()?;
    // SAFETY: Slot 11 of IPointerRoutedEventArgs is GetCurrentPoint, slot 8 of IPointerPoint is
    // get_PointerDeviceType with 2 for a mouse, slot 10 is get_Position, slot 11 is
    // get_Properties and slot 13 of IPointerPointProperties is get_IsLeftButtonPressed.
    unsafe {
        let point =
            out(|raw| call!(args, 11, element.as_raw() => *mut c_void, raw => *mut *mut c_void))
                .ok()?;
        let mut device = 0i32;
        call!(point, 8, &mut device => *mut i32);
        let properties = point.get(11).ok()?;
        let mut pressed = 0u8;
        call!(properties, 13, &mut pressed => *mut u8);
        if device != 2 || pressed == 0 {
            return None;
        }
        let mut position = Point::default();
        call!(point, 10, &mut position => *mut Point);
        Some((position, args))
    }
}
