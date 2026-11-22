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

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::OnceLock;

use imageview_core::{MAXIMUM_MAGNIFICATION, MINIMUM_MAGNIFICATION};

#[cfg(feature = "edit")]
pub(crate) use self::edit::Mark;
use super::composition::{MediaVisual, Surface};
use super::headers::*;
use super::winrt::*;

#[cfg(feature = "edit")]
mod edit;

/// The height of the header bar, the tall caption buttons of the window fill it.
pub(crate) const HEADER_HEIGHT: f64 = 48.0;
/// The width of the sidebar with the pages of a file and of the inspector, beside the image.
pub(crate) const SIDEBAR_WIDTH: f64 = 168.0;
pub(crate) const INSPECTOR_WIDTH: f64 = 260.0;

thread_local! {
    static RUNTIME: RefCell<Option<(ComPtr, ComPtr, ComPtr)>> = const { RefCell::new(None) };
}

/// Finish XAML work before the event loop uninitializes COM.
pub(crate) fn shutdown() {
    let runtime = RUNTIME.with_borrow_mut(Option::take);
    if let Some((dispatcher, manager, application)) = runtime {
        if let Ok(closable) = manager.cast(&IID_ICLOSABLE) {
            // SAFETY: The XAML manager was queried for IClosable on its owning thread.
            unsafe { call!(closable, IClosable::Close) };
        }
        let iid = GUID::from_u128(0x4c68ee2a_1cb1_5591_a3a2_9b590b8f8b9a);
        if let Ok(controller) = dispatcher.cast(&iid) {
            // SAFETY: The controller was queried on its owning thread. Shutting it down
            // drains the queue, including the asynchronous XAML shutdown.
            unsafe { call!(controller, IDispatcherQueueController2::ShutdownQueue) };
        }
        drop((application, manager, dispatcher));
    }
}

// The oldest Windows App SDK release ImageView supports, cargo-bundle writes the same
// dependency into the MSIX manifest.
const FRAMEWORK_FAMILY: &[u16] = w!("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe");
const MIN_VERSION: u64 = 2 << 48;
const MIN_VERSION_NAME: &str = "2.0";
const DOWNLOAD_URL: &str = "https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads";

const STATEREPOSITORY_E_DEPENDENCY_NOT_RESOLVED: HRESULT = 0x8067_0016_u32 as i32;

// The header bar buttons and the application actions they perform.
const VIEW_ACTIONS: &[(&str, &str)] = &[
    ("Previous", "view.previous"),
    ("Next", "view.next"),
    ("ZoomOut", "view.zoom_out"),
    ("ResetZoom", "view.zoom_to_fit"),
    ("ZoomIn", "view.zoom_in"),
    ("SidebarButton", "view.sidebar"),
    ("InfoButton", "view.info"),
];

// The scroll viewer zooms from 0.1, so it shows the media at this scale and zooms that by the
// magnification divided by it, which keeps the smallest magnification in its range.
const MIN_ZOOM_FACTOR: f64 = 0.1;
const CONTENT_SCALE: f64 = MINIMUM_MAGNIFICATION / MIN_ZOOM_FACTOR;
const MAX_ZOOM_FACTOR: f64 = MAXIMUM_MAGNIFICATION / CONTENT_SCALE;

// Where the buttons start, after the app icon.
const BUTTONS_LEFT: f64 = 44.0;
// The space between the last buttons and the caption buttons.
const END_GAP: f64 = 8.0;
// The space between groups of buttons.
const GROUP_SPACING: f64 = 8.0;

/// The narrowest header bar that fits the icon, the sidebar and navigation buttons, a trimmed
/// title area, the zoom, Info and Edit buttons and the tall caption buttons, so they never overlap.
/// The edit groups that do not fit move to the More menu.
pub(crate) const MIN_WIDTH: f64 = BUTTONS_LEFT
    // The sidebar button.
    + 40.0 + GROUP_SPACING
    // Previous and next, with a divider and a border.
    + 2.0 * 40.0 + 3.0
    // The title can shrink away at the minimum width.
    // The zoom out, reset and zoom in buttons, with dividers and a border.
    + 40.0 + 56.0 + 40.0 + 4.0
    // The Info button.
    + GROUP_SPACING + 40.0
    // The Edit button.
    + if cfg!(feature = "edit") { GROUP_SPACING + 56.0 } else { 0.0 }
    + END_GAP
    // The three tall caption buttons.
    + 3.0 * 48.0;

// The whole window: one header bar, like a GTK header bar or a unified macOS toolbar, with the
// app icon and navigation buttons, the centered title and the zoom and Edit buttons before the
// caption buttons, or the edit groups and the Done button, above a scroll viewer that zooms and
// pans the media over a checkerboard.
const XAML: &str = include_str!("winui/winui.xaml");

// The XAML of the edit controls, empty in a build without editing.
#[cfg(feature = "edit")]
macro_rules! edit_xaml {
    ($file:literal) => {
        include_str!(concat!("winui/", $file))
    };
}
#[cfg(not(feature = "edit"))]
macro_rules! edit_xaml {
    ($file:literal) => {
        ""
    };
}

// The edit controls with the placeholders of the XAML they take the place of.
const EDIT_XAML: [(&str, &str); 5] = [
    ("{EDIT_START}", edit_xaml!("edit_start.xaml")),
    ("{EDIT_BUTTON}", edit_xaml!("edit_button.xaml")),
    ("{EDIT_END}", edit_xaml!("edit_end.xaml")),
    ("{OVERLAY}", edit_xaml!("overlay.xaml")),
    ("{TOOL}", edit_xaml!("tool.xaml")),
];

// The header bar buttons and menu items and the application actions they perform.
fn actions() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    let actions = VIEW_ACTIONS.iter();
    #[cfg(feature = "edit")]
    let actions = actions.chain(edit::EDIT_ACTIONS);
    actions
}

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
                w!("Windows App Runtime Required"),
                MB_YESNO | MB_ICONWARNING,
            ),
            Self::Unsupported => (
                "ImageView needs Windows 11 to load the Windows App Runtime.".to_owned(),
                w!("ImageView"),
                MB_OK | MB_ICONERROR,
            ),
            Self::Failed(component, result) => (
                format!(
                    "Failed to create the WinUI {component} (HRESULT 0x{:08X}).",
                    *result as u32
                ),
                w!("WinUI Error"),
                MB_OK | MB_ICONERROR,
            ),
        };
        let text = wide(&text);
        // SAFETY: The strings outlive the message box.
        let answer = unsafe { MessageBoxW(hwnd, text.as_ptr(), caption.as_ptr(), kind) };
        if matches!(self, Self::Missing) && answer == IDYES {
            bwindow::open_url(DOWNLOAD_URL);
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
    // and go when the thread keeps its XAML manager. Shut both down before COM uninitializes.
    // SAFETY: The factory IID selects the dispatcher statics for this thread.
    let dispatcher = factory(
        w!("Microsoft.UI.Dispatching.DispatcherQueueController"),
        &IID_IDISPATCHER_QUEUE_CONTROLLER_STATICS,
    )
    .and_then(|statics| unsafe {
        out(|raw| {
            call!(
                statics,
                IDispatcherQueueControllerStatics::CreateOnCurrentThread,
                raw
            )
        })
    })
    .map_err(failed("dispatcher queue"))?;
    // The application comes first, otherwise XAML starts with a default one.
    let application = create_application().map_err(failed("application"))?;
    // SAFETY: The factory IID selects the XAML manager statics for this thread.
    let manager = factory(
        w!("Microsoft.UI.Xaml.Hosting.WindowsXamlManager"),
        &IID_IWINDOWS_XAML_MANAGER_STATICS,
    )
    .and_then(|statics| unsafe {
        out(|raw| {
            call!(
                statics,
                IWindowsXamlManagerStatics::InitializeForCurrentThread,
                raw
            )
        })
    })
    .map_err(failed("XAML manager"))?;
    // The Fluent styles of the WinUI controls, like the App.xaml of a WinUI app has.
    let resources = activate(w!("Microsoft.UI.Xaml.Controls.XamlControlsResources"))
        .and_then(|resources| resources.cast(&IID_IRESOURCE_DICTIONARY))
        .map_err(failed("controls resources"))?;
    // SAFETY: The application and resource dictionary live on this XAML thread.
    unsafe {
        check(call!(
            application,
            IApplication::SetResources,
            resources.as_raw()
        ))
    }
    .map_err(failed("controls resources"))?;
    RUNTIME.with_borrow_mut(|runtime| *runtime = Some((dispatcher, manager, application)));
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
        let kernelbase = GetModuleHandleW(w!("kernelbase.dll").as_ptr());
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
    let family = FRAMEWORK_FAMILY;
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
            let library = LoadLibraryW(w!("Microsoft.Internal.FrameworkUdk.dll").as_ptr());
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
            w!("Microsoft.UI.Xaml.Media.Imaging.WriteableBitmap"),
            &IID_IWRITEABLE_BITMAP_FACTORY,
        )
        .ok()?;
        let (width, height) = (i32::try_from(width).ok()?, i32::try_from(height).ok()?);
        // SAFETY: The factory was queried for IWriteableBitmapFactory and out supplies writable storage.
        let bitmap = out(|raw| unsafe {
            call!(
                factory,
                IWriteableBitmapFactory::CreateInstanceWithDimensions,
                width,
                height,
                raw
            )
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
        self.update(|buffer| buffer.copy_from_slice(pixels));
    }

    /// Convert canvas pixels directly into the WinUI buffer without a second full-size image.
    pub(crate) fn write_rgba(&self, pixels: &[u8]) {
        let length = self.size.0 as usize * self.size.1 as usize * 4;
        assert_eq!(pixels.len(), length, "bitmap pixels have the wrong size");
        self.update(|buffer| premultiplied_bgra(buffer, pixels));
    }

    fn update(&self, write: impl FnOnce(&mut [u8])) {
        let length = self.size.0 as usize * self.size.1 as usize * 4;
        // SAFETY: The owned bitmap and pixel buffer keep the byte pointer live during
        // the write. The slice length is the bitmap extent times four bytes per pixel.
        unsafe {
            let Ok(buffer) = out(|raw| call!(self.bitmap, IWriteableBitmap::GetPixelBuffer, raw))
                .and_then(|buffer| buffer.cast(&IID_IBUFFER_BYTE_ACCESS))
            else {
                return;
            };
            let mut bytes = null_mut::<u8>();
            if check(call!(buffer, IBufferByteAccess::Buffer, &mut bytes)).is_ok()
                && !bytes.is_null()
            {
                write(std::slice::from_raw_parts_mut(bytes, length));
                call!(self.bitmap, IWriteableBitmap::Invalidate);
            }
        }
    }
}

/// Converts RGBA pixels, like those of a canvas, into premultiplied BGRA pixels.
pub(crate) fn premultiplied_bgra(target: &mut [u8], rgba: &[u8]) {
    for (dest, source) in target
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(rgba.as_chunks::<4>().0)
    {
        let [red, green, blue, alpha] = *source;
        let premultiply = |channel: u8| (u16::from(channel) * u16::from(alpha) / 255) as u8;
        *dest = [
            premultiply(blue),
            premultiply(green),
            premultiply(red),
            alpha,
        ];
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

/// Boxes a property value and queries its IReference interface.
fn boxed(iid: &GUID, create: impl FnOnce(&ComPtr, *mut *mut c_void) -> HRESULT) -> Option<ComPtr> {
    let statics = factory(
        w!("Windows.Foundation.PropertyValue"),
        &IID_IPROPERTY_VALUE_STATICS,
    )
    .ok()?;
    out(|raw| create(&statics, raw))
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
    media_visual: MediaVisual,
    checkerboard: ComPtr,
    spinner: ComPtr,
    // The panels beside the image, their contents and the buttons that open them.
    panels: [PanelElements; 2],
    regions: Rc<Regions>,
    // The edit controls, the overlay of the crop rectangle or selection and the tool card.
    #[cfg(feature = "edit")]
    edit: edit::EditElements,
    // A view change asked for before the scroll viewer was laid out, which it would ignore.
    pending: Rc<Cell<Option<View>>>,
    // Whether the window draws the Mica backdrop itself, under the transparent island.
    window_backdrop: bool,
    // The composition target statics and the registration of the rendered frame handler,
    // until the first frame is rendered.
    rendered: Cell<Option<(ComPtr, i64)>>,
}

/// A panel beside the image.
#[derive(Clone, Copy)]
pub(crate) enum Panel {
    /// The sidebar with the pages of a file with several.
    Pages,
    /// The inspector with the information about the image.
    Info,
}

// The elements of a panel: the panel, the border that holds its content and its toggle button.
struct PanelElements {
    panel: ComPtr,
    host: ComPtr,
    button: ComPtr,
}

// The button groups in the header bar, which take clicks instead of dragging the window.
struct Regions {
    non_client: ComPtr,
    title_bar: ComPtr,
    start: ComPtr,
    end: ComPtr,
    // The edit groups, some of which move to the More menu while editing.
    #[cfg(feature = "edit")]
    edit: edit::EditRegions,
    // The width and header bar height in physical pixels and the scale.
    size: Cell<(i32, i32, f64)>,
}

// The width of an element in logical pixels.
fn actual_width(element: &ComPtr) -> f64 {
    let mut width = 0.0;
    // SAFETY: The caller supplies an IFrameworkElement on this XAML thread.
    unsafe { call!(element, IFrameworkElement::GetActualWidth, &mut width) };
    width
}

impl Regions {
    fn update(&self) {
        let (width, height, scale) = self.size.get();
        if width == 0 {
            return;
        }
        let mut inset = 0;
        // SAFETY: The title bar and end group retain their queried interfaces on this thread.
        unsafe {
            call!(
                self.title_bar,
                IAppWindowTitleBar::GetRightInset,
                &mut inset
            );
            let margin = Thickness {
                left: 0.0,
                top: 0.0,
                right: f64::from(inset) / scale + END_GAP,
                bottom: 0.0,
            };
            call!(self.end, IFrameworkElement::SetMargin, margin);
        }
        #[cfg(feature = "edit")]
        self.edit.update(f64::from(width - inset) / scale);
        let physical = |element: &ComPtr| (actual_width(element) * scale).round() as i32;
        let (start, end) = (physical(&self.start), physical(&self.end));
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
        // SAFETY: The title-bar region array lives through the call and the count matches
        // its length. Region kind 9 passes input through to XAML.
        unsafe {
            call!(
                self.non_client,
                IInputNonClientPointerSource::SetRegionRects,
                9i32,
                rects.len() as u32,
                rects.as_ptr()
            )
        };
    }
}

// Where a drag with the primary button started: the pointer and the scroll offsets.
type Drag = (Point, (f64, f64));

// Scroll offsets and a magnification, keeping what is not given.
type View = (Option<f64>, Option<f64>, Option<f64>);

/// What follows the input of an island.
pub(crate) struct Handlers {
    /// Runs the application action of a button or menu item.
    pub(crate) action: Box<dyn Fn(&'static str)>,
    /// Follows every change of the zoom or scroll position, with whether it settled.
    pub(crate) view: Box<dyn Fn(bool)>,
    /// Takes drags that crop or select first, returns whether it does.
    #[cfg(feature = "edit")]
    pub(crate) mark: Box<dyn Fn(Mark) -> bool>,
    /// Follows the first rendered frame, until [`Island::stop_rendered`].
    pub(crate) rendered: Box<dyn Fn()>,
}

/// Creates the object of a XAML document.
pub(crate) fn load_xaml(xaml: &str) -> Result<ComPtr> {
    let statics = factory(
        w!("Microsoft.UI.Xaml.Markup.XamlReader"),
        &IID_IXAML_READER_STATICS,
    )?;
    let xaml = HString::new(xaml);
    // SAFETY: The reader statics match the factory IID and the HSTRING lives through the call.
    out(|raw| unsafe { call!(statics, IXamlReaderStatics::Load, xaml.as_raw(), raw) })
}

const IID_ICONTENT_DIALOG: GUID = GUID::from_u128(0xac2145a3_4a32_5305_a81d_47509515bfce);
const IID_ICONTENT_DIALOG_CLOSED_EVENT_ARGS: GUID =
    GUID::from_u128(0x9b84e681_1ab6_5485_88b2_d0d3c05b29f3);
// TypedEventHandler<ContentDialog, ContentDialogClosedEventArgs>.
const IID_DIALOG_CLOSED_HANDLER: GUID = GUID::from_u128(0xaa8730ec_c9bd_50ea_8714_da30c29590d9);

/// Text escaped for a XAML attribute, without the characters XML does not allow, like control
/// characters of image metadata. A leading brace would start a markup extension, `{}` keeps it
/// text.
pub(crate) fn escape_xml(text: &str) -> String {
    let text: String = text
        .chars()
        .filter(|character| {
            matches!(character, '\t' | '\n' | '\r' | ' '..='\u{d7ff}' | '\u{e000}'..='\u{fffd}')
                || u32::from(*character) >= 0x10000
        })
        .collect();
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    if escaped.starts_with('{') {
        format!("{{}}{escaped}")
    } else {
        escaped
    }
}

/// An asynchronous ContentDialog attached to this island's XamlRoot.
pub(super) struct Dialog {
    content: ComPtr,
    _operation: ComPtr,
    closed: i64,
}

impl Dialog {
    fn new(
        root: &ComPtr,
        title: &str,
        description: &str,
        save: bool,
        answered: impl Fn(i32) + 'static,
    ) -> Result<Self> {
        let buttons = if save {
            r#"PrimaryButtonText="Save" SecondaryButtonText="Don't Save" CloseButtonText="Cancel" DefaultButton="Primary""#
        } else {
            r#"CloseButtonText="Close" DefaultButton="Close""#
        };
        let xaml = format!(
            r#"<ContentDialog xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" Title="{}" {buttons}><TextBlock Text="{}" TextWrapping="Wrap" IsTextSelectionEnabled="True" /></ContentDialog>"#,
            escape_xml(title),
            escape_xml(description)
        );
        let content = load_xaml(&xaml)?.cast(&IID_ICONTENT_DIALOG)?;
        let element = content.cast(&IID_IUI_ELEMENT)?;
        let root = root.cast(&IID_IUI_ELEMENT)?;
        // SAFETY: Both elements were queried for IUIElement on the same XAML thread.
        unsafe {
            let xaml_root = out(|raw| call!(root, IUIElement::GetXamlRoot, raw))?;
            check(call!(element, IUIElement::SetXamlRoot, xaml_root.as_raw()))?;
            let root_theme = root.cast(&IID_IFRAMEWORK_ELEMENT)?;
            let dialog_theme = element.cast(&IID_IFRAMEWORK_ELEMENT)?;
            let mut theme = 0i32;
            check(call!(
                root_theme,
                IFrameworkElement::GetRequestedTheme,
                &mut theme
            ))?;
            check(call!(
                dialog_theme,
                IFrameworkElement::SetRequestedTheme,
                theme
            ))?;
        }
        let handler = event_handler(IID_DIALOG_CLOSED_HANDLER, move |_, args| {
            let mut result = 0i32;
            if let Some(args) =
                args.and_then(|args| args.cast(&IID_ICONTENT_DIALOG_CLOSED_EVENT_ARGS).ok())
            {
                // SAFETY: The event arguments were queried for the closed-event interface.
                unsafe { call!(args, IContentDialogClosedEventArgs::GetResult, &mut result) };
            }
            answered(result);
        });
        let mut closed = 0;
        // SAFETY: The dialog retains the matching delegate and writes its token to closed.
        let operation = unsafe {
            check(call!(
                content,
                IContentDialog::AddClosed,
                handler.as_raw(),
                &mut closed
            ))?;
            out(|raw| call!(content, IContentDialog::ShowAsync, raw))?
        };
        Ok(Self {
            content,
            _operation: operation,
            closed,
        })
    }

    pub(super) fn hide(&self) {
        // SAFETY: The dialog owns its IContentDialog reference on this XAML thread.
        unsafe { call!(self.content, IContentDialog::Hide) };
    }
}

impl Drop for Dialog {
    fn drop(&mut self) {
        // SAFETY: Unsubscribe before Hide, which can synchronously raise Closed during teardown.
        unsafe {
            call!(self.content, IContentDialog::RemoveClosed, self.closed);
        }
        self.hide();
    }
}

/// Finds an element by its name in a XAML tree, as an interface.
pub(crate) fn find_name(root: &ComPtr, name: &[u16], iid: &GUID) -> Result<ComPtr> {
    let name = HString::from_wide(name);
    // SAFETY: The queried framework element and name HSTRING live through the call.
    out(|raw| unsafe { call!(root, IFrameworkElement::FindName, name.as_raw(), raw) })
        .and_then(|element| element.cast(iid))
}

/// Shows or collapses an element.
pub(crate) fn set_visible(element: &ComPtr, visible: bool) {
    if let Ok(element) = element.cast(&IID_IUI_ELEMENT) {
        // SAFETY: The query succeeded and the visibility value is Visible (0) or Collapsed (1).
        unsafe { call!(element, IUIElement::SetVisibility, i32::from(!visible)) };
    }
}

/// Boxes a boolean as an `IReference<Boolean>`.
pub(crate) fn boxed_bool(value: bool) -> Option<ComPtr> {
    boxed(&IID_IREFERENCE_BOOLEAN, |statics, out| {
        // SAFETY: boxed supplies the property-value statics and a writable output pointer.
        unsafe { call!(statics, IPropertyValueStatics::CreateBoolean, value, out) }
    })
}

/// Adds an event handler with a typed vtable entry.
macro_rules! add_event {
    ($object:expr, $interface:ident::$method:ident, $handler:expr $(,)?) => {{
        let object = &$object;
        let handler = &$handler;
        let mut token = 0;
        // SAFETY: The caller supplies the named interface and its matching event delegate.
        check(unsafe { call!(object, $interface::$method, handler.as_raw(), &mut token) })
    }};
}
pub(crate) use add_event;

impl Island {
    pub(super) fn show_alert(
        &self,
        title: &str,
        description: &str,
        save: bool,
        answered: impl Fn(i32) + 'static,
    ) -> Result<Dialog> {
        Dialog::new(&self.root, title, description, save, answered)
    }
    /// Creates the island in `parent`, which fills the client area of `window`, following its
    /// input with `handlers`. With `window_backdrop` the window also draws Mica itself, under the
    /// island.
    pub(crate) fn new(
        parent: HWND,
        window: HWND,
        dark: bool,
        window_backdrop: bool,
        handlers: Handlers,
    ) -> Self {
        Self::create(parent, window, window_backdrop, handlers)
            .inspect(|island| island.set_dark(dark))
            .unwrap_or_else(|(component, result)| fail(window, component, result))
    }

    fn create(
        parent: HWND,
        window: HWND,
        window_backdrop: bool,
        handlers: Handlers,
    ) -> std::result::Result<Self, (&'static str, HRESULT)> {
        let Handlers {
            action: on_action,
            view: on_view,
            #[cfg(feature = "edit")]
                mark: on_mark,
            rendered: on_rendered,
        } = handlers;
        let failed = |component| move |result| (component, result);

        // The content extends into the title bar, under the caption buttons.
        let title_bar = factory(
            w!("Microsoft.UI.Windowing.AppWindow"),
            &IID_IAPP_WINDOW_STATICS,
        )
        .and_then(|statics| {
            let id = window_id(window)?;
            // SAFETY: The activation factory supplies the app-window statics for this window ID.
            let app_window =
                out(|raw| unsafe { call!(statics, IAppWindowStatics::GetFromWindowId, id, raw) })?;
            // SAFETY: The statics returned an IAppWindow and out supplies writable storage.
            unsafe { out(|raw| call!(app_window, IAppWindow::GetTitleBar, raw)) }
        })
        .map_err(failed("title bar"))?;
        // SAFETY: The title-bar interfaces are live on the window thread. Height option 1
        // accommodates the taller caption buttons.
        unsafe {
            check(call!(
                title_bar,
                IAppWindowTitleBar::SetExtendsContentIntoTitleBar,
                1u8
            ))
            .map_err(failed("title bar"))?;
            let title_bar2 = title_bar
                .cast(&IID_IAPP_WINDOW_TITLE_BAR2)
                .map_err(failed("title bar"))?;
            check(call!(
                title_bar2,
                IAppWindowTitleBar2::SetPreferredHeightOption,
                1i32
            ))
            .map_err(failed("title bar"))?;
        }

        // SAFETY: The source is created on this XAML thread and initialized before accessing
        // its site bridge. The bridge is queried for IDesktopSiteBridge before it is shown.
        let (source, bridge) = unsafe {
            let source = compose(
                w!("Microsoft.UI.Xaml.Hosting.DesktopWindowXamlSource"),
                &IID_IDESKTOP_WINDOW_XAML_SOURCE_FACTORY,
            )
            .map_err(failed("XAML island"))?;
            let id = window_id(parent).map_err(failed("XAML island"))?;
            check(call!(source, IDesktopWindowXamlSource::Initialize, id))
                .map_err(failed("XAML island"))?;
            let bridge = out(|raw| call!(source, IDesktopWindowXamlSource::GetSiteBridge, raw))
                .and_then(|bridge| bridge.cast(&IID_IDESKTOP_SITE_BRIDGE))
                .map_err(failed("XAML island"))?;
            check(call!(bridge, IDesktopSiteBridge::Show)).map_err(failed("XAML island"))?;
            (source, bridge)
        };

        let xaml = EDIT_XAML
            .iter()
            .fold(XAML.to_owned(), |xaml, (placeholder, part)| {
                xaml.replace(placeholder, part)
            })
            .replace("{MIN_ZOOM}", &MIN_ZOOM_FACTOR.to_string())
            .replace("{MAX_ZOOM}", &MAX_ZOOM_FACTOR.to_string())
            .replace("{SIDEBAR_WIDTH}", &SIDEBAR_WIDTH.to_string())
            .replace("{INSPECTOR_WIDTH}", &INSPECTOR_WIDTH.to_string());
        let root = load_xaml(&xaml)
            .and_then(|root| root.cast(&IID_IFRAMEWORK_ELEMENT))
            .map_err(failed("window content"))?;
        let element = root
            .cast(&IID_IUI_ELEMENT)
            .map_err(failed("window content"))?;
        // SAFETY: The XAML source and its new content live on the same thread.
        unsafe {
            check(call!(
                source,
                IDesktopWindowXamlSource::SetContent,
                element.as_raw()
            ))
        }
        .map_err(failed("window content"))?;

        let find = |name: &[u16], iid: &GUID| find_name(&root, name, iid);

        // Mica shows through the header bar where Windows supports it, otherwise it gets a solid
        // background. The backdrop needs the content of the island to attach to. Where the
        // window also draws Mica itself, that fills the pixels a resize uncovers before the
        // island catches up. The island keeps its own backdrop then too: closing an island
        // without one fails fast in CoreUIComponents.dll once a file dialog has been shown.
        let mica = compose(
            w!("Microsoft.UI.Xaml.Media.MicaBackdrop"),
            &IID_IMICA_BACKDROP_FACTORY,
        )
        .and_then(|mica| mica.cast(&IID_ISYSTEM_BACKDROP))
        // SAFETY: The backdrop was queried for ISystemBackdrop and lives through the call.
        .and_then(|mica| unsafe {
            check(call!(
                source,
                IDesktopWindowXamlSource::SetSystemBackdrop,
                mica.as_raw()
            ))
        });
        if mica.is_err() {
            let background =
                find(w!("Background"), &IID_IUI_ELEMENT).map_err(failed("window content"))?;
            // SAFETY: find queried the background for IUIElement. Visibility 0 shows it.
            unsafe { call!(background, IUIElement::SetVisibility, 0i32) };
        }
        let title = find(w!("Title"), &IID_ITEXT_BLOCK).map_err(failed("window content"))?;
        let subtitle = find(w!("Subtitle"), &IID_ITEXT_BLOCK).map_err(failed("window content"))?;
        let heading = find(w!("Heading"), &IID_IUI_ELEMENT).map_err(failed("window content"))?;
        // SAFETY: The window is alive.
        let scale = f64::from(unsafe { GetDpiForWindow(window) }.max(96)) / 96.0;
        if let Some(icon) = app_icon((16.0 * scale).round() as i32) {
            let image = find(w!("Icon"), &IID_IIMAGE).map_err(failed("window content"))?;
            // SAFETY: find queried the icon element for IImage, and the bitmap source is live.
            _ = unsafe { check(call!(image, IImage::SetSource, icon.source.as_raw())) };
        }
        let zoom = find(w!("Zoom"), &IID_ITEXT_BLOCK).map_err(failed("window content"))?;
        let start = find(w!("Start"), &IID_IFRAMEWORK_ELEMENT).map_err(failed("window content"))?;
        let end = find(w!("End"), &IID_IFRAMEWORK_ELEMENT).map_err(failed("window content"))?;
        let scroller =
            find(w!("Scroller"), &IID_ISCROLL_VIEWER).map_err(failed("window content"))?;
        let media = find(w!("Media"), &IID_IFRAMEWORK_ELEMENT).map_err(failed("window content"))?;
        let media_visual = MediaVisual::new(&media).map_err(failed("media visual"))?;
        let checkerboard =
            find(w!("Checkerboard"), &IID_IIMAGE).map_err(failed("window content"))?;
        let panels = [
            (w!("PagesPanel"), w!("PagesHost"), w!("SidebarButton")),
            (w!("InfoPanel"), w!("InfoHost"), w!("InfoButton")),
        ]
        .map(|(panel, host, button)| -> Result<PanelElements> {
            Ok(PanelElements {
                panel: find(panel, &IID_IUI_ELEMENT)?,
                host: find(host, &IID_IBORDER)?,
                button: find(button, &IID_ITOGGLE_BUTTON)?,
            })
        });
        let [pages_panel, info_panel] = panels;
        let panels = [
            pages_panel.map_err(failed("window content"))?,
            info_panel.map_err(failed("window content"))?,
        ];
        let spinner = find(w!("Spinner"), &IID_IUI_ELEMENT).map_err(failed("window content"))?;
        #[cfg(feature = "edit")]
        let edit = edit::EditElements::find(&root).map_err(failed("window content"))?;

        // The buttons in the title bar take clicks, the rest of it drags the window.
        let non_client = factory(
            w!("Microsoft.UI.Input.InputNonClientPointerSource"),
            &IID_IINPUT_NON_CLIENT_POINTER_SOURCE_STATICS,
        )
        .and_then(|statics| {
            let id = window_id(window)?;
            // SAFETY: The factory supplies the pointer-source statics for this window ID.
            out(|raw| unsafe {
                call!(
                    statics,
                    IInputNonClientPointerSourceStatics::GetForWindowId,
                    id,
                    raw
                )
            })
        })
        .map_err(failed("title bar"))?;
        let regions = Rc::new(Regions {
            non_client,
            title_bar: title_bar.clone(),
            start: start.clone(),
            end: end.clone(),
            #[cfg(feature = "edit")]
            edit: edit::EditRegions::find(&root).map_err(failed("title bar"))?,
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
            // SAFETY: Each group is an IFrameworkElement and retains the size-changed delegate.
            check(unsafe {
                call!(
                    group,
                    IFrameworkElement::AddSizeChanged,
                    resized.as_raw(),
                    &mut token
                )
            })
            .map_err(failed("title bar"))?;
        }

        let on_action: Rc<dyn Fn(&'static str)> = Rc::from(on_action);
        let clicked = {
            let on_action = on_action.clone();
            #[cfg(feature = "edit")]
            let (opened, toggled) = (edit.tool_open.clone(), edit.toggled.clone());
            event_handler(IID_ROUTED_EVENT_HANDLER, move |sender, _| {
                let Ok(sender) = sender.cast(&IID_IFRAMEWORK_ELEMENT) else {
                    return;
                };
                let mut name = null_mut();
                // SAFETY: The sender was queried for IFrameworkElement and name receives an owned HSTRING.
                if check(unsafe { call!(sender, IFrameworkElement::GetName, &mut name) }).is_err() {
                    return;
                }
                // SAFETY: The name is owned by this call.
                let name = unsafe { HString::from_raw(name) }.to_string_lossy();
                let Some((_, action)) = actions().find(|(button, _)| *button == name) else {
                    return;
                };
                #[cfg(feature = "edit")]
                if !edit::clicked(&sender, &name, action, &toggled, &opened) {
                    return;
                }
                on_action(action);
            })
        };
        for (name, _) in actions() {
            let element =
                find(&wide(name), &IID_IFRAMEWORK_ELEMENT).map_err(failed("window content"))?;
            match element.cast(&IID_IBUTTON_BASE) {
                Ok(button) => add_event!(&button, IButtonBase::AddClick, &clicked),
                Err(_) => element
                    .cast(&IID_IMENU_FLYOUT_ITEM)
                    .and_then(|item| add_event!(&item, IMenuFlyoutItem::AddClick, &clicked)),
            }
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
            // SAFETY: The event sender was queried for IScrollViewer and factor is writable.
            unsafe { call!(scroller, IScrollViewer::GetZoomFactor, &mut factor) };
            let magnification = f64::from(factor) * CONTENT_SCALE;
            let label = HString::new(&format!("{:.0}%", magnification * 100.0));
            // SAFETY: The captured zoom label is an ITextBlock and its new text lives through the call.
            _ = unsafe { check(call!(zoom, ITextBlock::SetText, label.as_raw())) };
            let mut intermediate = 0u8;
            if let Some(args) =
                args.and_then(|args| args.cast(&IID_ISCROLL_VIEWER_VIEW_CHANGED_EVENT_ARGS).ok())
            {
                // SAFETY: The arguments were queried for IScrollViewerViewChangedEventArgs.
                unsafe {
                    call!(
                        args,
                        IScrollViewerViewChangedEventArgs::GetIsIntermediate,
                        &mut intermediate
                    )
                };
            }
            on_view(intermediate == 0);
        });
        let mut token = 0i64;
        // SAFETY: The scroll viewer retains the matching view-changed delegate.
        check(unsafe {
            call!(
                scroller,
                IScrollViewer::AddViewChanged,
                view_changed.as_raw(),
                &mut token
            )
        })
        .map_err(failed("window content"))?;

        Self::add_drag(
            &scroller,
            #[cfg(feature = "edit")]
            on_mark,
        )
        .map_err(failed("window content"))?;
        #[cfg(feature = "edit")]
        edit::add_escape(&scroller, edit.tool_open.clone(), on_action)
            .map_err(failed("window content"))?;

        let on_rendered: Rc<dyn Fn()> = Rc::from(on_rendered);
        // The view change from before the first layout applies once the scroll viewer has a size.
        let pending = Rc::new(Cell::<Option<View>>::new(None));
        let laid_out = {
            let pending = pending.clone();
            let reveal = on_rendered.clone();
            event_handler(IID_SIZE_CHANGED_EVENT_HANDLER, move |sender, _| {
                // The window can show once the island is laid out, it draws that layout in the
                // same frame.
                reveal();
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
        // SAFETY: The scroll viewer was queried for IFrameworkElement and retains the delegate.
        check(unsafe {
            call!(
                element,
                IFrameworkElement::AddSizeChanged,
                laid_out.as_raw(),
                &mut token
            )
        })
        .map_err(failed("window content"))?;

        // The window shows once the island has rendered its first frame, so it never shows the
        // empty window behind it.
        let statics = factory(
            w!("Microsoft.UI.Xaml.Media.CompositionTarget"),
            &IID_ICOMPOSITION_TARGET_STATICS,
        )
        .map_err(failed("window content"))?;
        let rendered = event_handler(IID_RENDERED_EVENT_HANDLER, move |_, _| on_rendered());
        let mut token = 0i64;
        // SAFETY: The composition target retains the rendered delegate until the token is removed.
        check(unsafe {
            call!(
                statics,
                ICompositionTargetStatics::AddRendered,
                rendered.as_raw(),
                &mut token
            )
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
            media_visual,
            checkerboard,
            spinner,
            panels,
            regions,
            #[cfg(feature = "edit")]
            edit,
            pending,
            window_backdrop,
            rendered: Cell::new(Some((statics, token))),
        })
    }

    /// Stops following the rendered frames.
    pub(crate) fn stop_rendered(&self) {
        if let Some((statics, token)) = self.rendered.take() {
            // SAFETY: The token came from a subscription on this same composition target.
            unsafe { call!(statics, ICompositionTargetStatics::RemoveRendered, token) };
        }
    }

    // Dragging with the primary mouse button crops or selects, or else pans the media, like the
    // hand tool of other viewers. Touch and touchpads pan with the scroll viewer itself. The
    // handlers get the scroll viewer as their sender, so they keep no reference to it that would
    // keep it alive.
    fn add_drag(
        scroller: &ComPtr,
        #[cfg(feature = "edit")] on_mark: impl Fn(Mark) -> bool + 'static,
    ) -> Result<()> {
        let drag = Rc::new(Cell::<Option<Drag>>::new(None));
        #[cfg(feature = "edit")]
        let marks = edit::Marks::new(on_mark);
        let pressed = {
            let drag = drag.clone();
            #[cfg(feature = "edit")]
            let marks = marks.clone();
            event_handler(IID_POINTER_EVENT_HANDLER, move |sender, args| {
                let (Some((point, args)), Ok(scroller), Ok(element)) = (
                    args.and_then(|args| mouse_drag_point(sender, args)),
                    sender.cast(&IID_ISCROLL_VIEWER),
                    sender.cast(&IID_IUI_ELEMENT),
                ) else {
                    return;
                };
                #[cfg(feature = "edit")]
                let marked = marks.begin(point);
                #[cfg(not(feature = "edit"))]
                let marked = false;
                if !marked {
                    drag.set(Some((point, scroll_offsets(&scroller))));
                }
                // SAFETY: The queried element and event arguments stay live while the pointer
                // returned by the event is captured.
                unsafe {
                    if let Ok(pointer) =
                        out(|raw| call!(args, IPointerRoutedEventArgs::GetPointer, raw))
                    {
                        let mut captured = 0u8;
                        call!(
                            element,
                            IUIElement::CapturePointer,
                            pointer.as_raw(),
                            &mut captured
                        );
                    }
                    call!(args, IPointerRoutedEventArgs::SetHandled, 1u8);
                }
            })
        };
        let moved = {
            let drag = drag.clone();
            #[cfg(feature = "edit")]
            let marks = marks.clone();
            event_handler(IID_POINTER_EVENT_HANDLER, move |sender, args| {
                let Some((point, _args)) = args.and_then(|args| mouse_drag_point(sender, args))
                else {
                    return;
                };
                #[cfg(feature = "edit")]
                if marks.drag(point, &_args) {
                    return;
                }
                let (Some((start, (x, y))), Ok(scroller)) =
                    (drag.get(), sender.cast(&IID_ISCROLL_VIEWER))
                else {
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
        let released = event_handler(IID_POINTER_EVENT_HANDLER, move |_, _| {
            drag.set(None);
            #[cfg(feature = "edit")]
            marks.end();
        });
        let element = scroller.cast(&IID_IUI_ELEMENT)?;
        add_event!(element, IUIElement::AddPointerPressed, pressed)?;
        add_event!(element, IUIElement::AddPointerMoved, moved)?;
        add_event!(element, IUIElement::AddPointerReleased, released)?;
        add_event!(element, IUIElement::AddPointerCaptureLost, released)?;
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
        // SAFETY: The site bridge belongs to this window thread and receives the rectangle by value.
        unsafe { call!(self.bridge, IDesktopSiteBridge::MoveAndResize, rect) };
        let header = ((HEADER_HEIGHT * scale).round() as i32).min(height);
        self.regions.size.set((width, header, scale));
        self.regions.update();
    }

    /// Shows the title and subtitle in the header bar.
    pub(crate) fn set_title(&self, title: &str, subtitle: &str) {
        for (text_block, text) in [(&self.title, title), (&self.subtitle, subtitle)] {
            let text = HString::new(text);
            // SAFETY: The title was queried for ITextBlock and text lives through the call.
            _ = unsafe { check(call!(text_block, ITextBlock::SetText, text.as_raw())) };
        }
    }

    /// Dims the title while the window is inactive, like the system title bar does.
    pub(crate) fn set_active(&self, active: bool) {
        // SAFETY: The heading retains its IUIElement interface on this XAML thread.
        unsafe {
            call!(
                self.heading,
                IUIElement::SetOpacity,
                if active { 1.0 } else { 0.5 }
            )
        };
    }

    /// The visual that shows the media, which makes its surfaces.
    pub(crate) const fn media_visual(&self) -> &MediaVisual {
        &self.media_visual
    }

    /// Shows a surface as the media, stretched to its size in logical pixels.
    pub(crate) fn set_media(&self, surface: &Surface, size: (f64, f64)) {
        let size = (size.0 * CONTENT_SCALE, size.1 * CONTENT_SCALE);
        set_size(&self.media, size);
        self.media_visual.show(surface, size);
    }

    /// Replaces the content of a panel.
    pub(crate) fn set_panel_content(&self, panel: Panel, content: &ComPtr) {
        let Ok(element) = content.cast(&IID_IUI_ELEMENT) else {
            return;
        };
        // SAFETY: The panel host is an IBorder and the child was queried for IUIElement.
        _ = unsafe {
            check(call!(
                self.panels[panel as usize].host,
                IBorder::SetChild,
                element.as_raw()
            ))
        };
    }

    /// Shows a panel beside the image, or hides it, with its button on or off.
    pub(crate) fn set_panel_open(&self, panel: Panel, open: bool) {
        let elements = &self.panels[panel as usize];
        set_visible(&elements.panel, open);
        if let Some(checked) = boxed_bool(open) {
            // SAFETY: The button retains its IToggleButton interface and checked owns the boxed boolean.
            _ = unsafe {
                check(call!(
                    elements.button,
                    IToggleButton::SetIsChecked,
                    checked.as_raw()
                ))
            };
        }
    }

    /// Shows the button of a panel, or hides it when there is nothing to show.
    pub(crate) fn set_panel_available(&self, panel: Panel, available: bool) {
        set_visible(&self.panels[panel as usize].button, available);
        self.regions.update();
    }

    /// Shows the spinning progress ring in the middle of the image, or hides it.
    pub(crate) fn set_spinner(&self, spinning: bool) {
        set_visible(&self.spinner, spinning);
    }

    /// Shows a bitmap as the checkerboard behind the media, centered at its size in logical
    /// pixels.
    pub(crate) fn set_checkerboard(&self, bitmap: &Bitmap, size: (f64, f64)) {
        set_image(&self.checkerboard, bitmap, size);
    }

    /// The magnification of the media.
    pub(crate) fn zoom_factor(&self) -> f64 {
        let mut factor = 1.0f32;
        // SAFETY: The island retains its IScrollViewer and factor is writable.
        unsafe { call!(self.scroller, IScrollViewer::GetZoomFactor, &mut factor) };
        f64::from(factor) * CONTENT_SCALE
    }

    /// The scroll offsets of the magnified media, in logical pixels.
    pub(crate) fn offsets(&self) -> (f64, f64) {
        scroll_offsets(&self.scroller)
    }

    #[cfg(all(test, feature = "edit"))]
    pub(super) fn scrollable_size(&self) -> (f64, f64) {
        let (mut width, mut height) = (0.0, 0.0);
        // SAFETY: The island retains its IScrollViewer and both outputs are writable.
        unsafe {
            call!(self.scroller, IScrollViewer::GetScrollableWidth, &mut width);
            call!(
                self.scroller,
                IScrollViewer::GetScrollableHeight,
                &mut height
            );
        }
        (width, height)
    }

    /// Scrolls and zooms right away, keeping what is not given, or once the scroll viewer is laid
    /// out.
    pub(crate) fn change_view(&self, x: Option<f64>, y: Option<f64>, zoom: Option<f64>) {
        let mut width = 0.0f64;
        // SAFETY: The island retains its IScrollViewer and width is writable.
        unsafe { call!(self.scroller, IScrollViewer::GetViewportWidth, &mut width) };
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
            // SAFETY: The element was queried for IUIElement. Focus state 3 is programmatic.
            unsafe { call!(element, IUIElement::Focus, 3i32, &mut focused) };
        }
    }

    /// Follows the light or dark theme, also with the caption buttons.
    pub(crate) fn set_dark(&self, dark: bool) {
        // SAFETY: The root retains its IFrameworkElement interface. Theme 1 is light and 2 is dark.
        unsafe {
            call!(
                self.root,
                IFrameworkElement::SetRequestedTheme,
                if dark { 2 } else { 1 }
            )
        };

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
        // resize, so it shows the Mica of the window or the background color of the theme
        // instead of white.
        let background = if self.window_backdrop {
            transparent
        } else {
            color(0xff, if dark { 0x20 } else { 0xf3 })
        };
        let title_bar = self.title_bar.as_raw().cast::<IAppWindowTitleBar>();
        // SAFETY: self.title_bar owns an IAppWindowTitleBar reference.
        let vtbl = unsafe { &*(*title_bar).vtbl };
        for (set_color, color) in [
            (vtbl.SetBackgroundColor, background),
            (vtbl.SetInactiveBackgroundColor, background),
            (vtbl.SetButtonBackgroundColor, transparent),
            (vtbl.SetButtonForegroundColor, foreground),
            (vtbl.SetButtonHoverBackgroundColor, hover),
            (vtbl.SetButtonHoverForegroundColor, foreground),
            (vtbl.SetButtonInactiveBackgroundColor, transparent),
            (vtbl.SetButtonInactiveForegroundColor, inactive),
            (vtbl.SetButtonPressedBackgroundColor, pressed),
            (vtbl.SetButtonPressedForegroundColor, foreground),
        ] {
            // SAFETY: Each setter takes IReference<Color>, which lives through the call.
            unsafe { set_color(title_bar, color_reference(color).as_raw()) };
        }
    }
}

impl Drop for Island {
    fn drop(&mut self) {
        self.stop_rendered();
        if let Ok(closable) = self.source.cast(&IID_ICLOSABLE) {
            // SAFETY: The XAML source was queried for IClosable on its owning thread.
            unsafe { call!(closable, IClosable::Close) };
        }
    }
}

/// Shows a bitmap in an image element at a size in logical pixels.
pub(crate) fn set_image(image: &ComPtr, bitmap: &Bitmap, size: (f64, f64)) {
    // SAFETY: The caller supplies an IImage, and the bitmap owns the IImageSource for the call.
    _ = unsafe { check(call!(image, IImage::SetSource, bitmap.source.as_raw())) };
    set_size(image, size);
}

// Sizes an element in logical pixels.
fn set_size(element: &ComPtr, (width, height): (f64, f64)) {
    if let Ok(element) = element.cast(&IID_IFRAMEWORK_ELEMENT) {
        // SAFETY: The element was queried for IFrameworkElement on this XAML thread.
        unsafe {
            call!(element, IFrameworkElement::SetWidth, width);
            call!(element, IFrameworkElement::SetHeight, height);
        }
    }
}

// Scrolls and zooms a scroll viewer right away, keeping what is not given.
fn change_view(scroller: &ComPtr, x: Option<f64>, y: Option<f64>, zoom: Option<f64>) {
    let offset = |value: f64| {
        boxed(&IID_IREFERENCE_DOUBLE, |statics, out| {
            // SAFETY: boxed supplies the property-value statics and a writable output.
            unsafe {
                call!(
                    statics,
                    IPropertyValueStatics::CreateDouble,
                    value.max(0.0),
                    out
                )
            }
        })
    };
    let x = x.and_then(offset);
    let y = y.and_then(offset);
    let zoom = zoom.and_then(|magnification| {
        let factor = (magnification / CONTENT_SCALE).clamp(MIN_ZOOM_FACTOR, MAX_ZOOM_FACTOR);
        // ChangeView takes a float. Rounding upward can put the fitted image just beyond
        // the viewport, which WinUI treats as scrollable even for a fraction of a pixel.
        let rounded = factor as f32;
        let rounded = if f64::from(rounded) > factor {
            rounded.next_down()
        } else {
            rounded
        };
        boxed(&IID_IREFERENCE_SINGLE, |statics, out| {
            // SAFETY: boxed supplies the property-value statics and a writable output.
            unsafe { call!(statics, IPropertyValueStatics::CreateSingle, rounded, out) }
        })
    });
    let raw = |value: &Option<ComPtr>| value.as_ref().map_or(null_mut(), ComPtr::as_raw);
    let mut changed = 0u8;
    // SAFETY: The caller supplies an IScrollViewer. The optional values are null or
    // live IReference values with the expected types for offsets and zoom.
    unsafe {
        call!(
            scroller,
            IScrollViewer::ChangeView,
            raw(&x),
            raw(&y),
            raw(&zoom),
            1u8,
            &mut changed
        )
    };
}

// The scroll offsets of a scroll viewer in logical pixels of the magnified media.
fn scroll_offsets(scroller: &ComPtr) -> (f64, f64) {
    let (mut x, mut y) = (0.0f64, 0.0f64);
    // SAFETY: The caller supplies an IScrollViewer and both offsets are writable.
    unsafe {
        call!(scroller, IScrollViewer::GetHorizontalOffset, &mut x);
        call!(scroller, IScrollViewer::GetVerticalOffset, &mut y);
    }
    (x, y)
}

// The position of a mouse pointer with the primary button down relative to the element that got
// the event, and the event arguments.
fn mouse_drag_point(element: &ComPtr, args: &ComPtr) -> Option<(Point, ComPtr)> {
    let args = args.cast(&IID_IPOINTER_ROUTED_EVENT_ARGS).ok()?;
    let element = element.cast(&IID_IUI_ELEMENT).ok()?;
    // SAFETY: The queried event arguments and element are live on this XAML thread.
    // Each returned pointer object stays owned while its properties are read.
    unsafe {
        let point = out(|raw| {
            call!(
                args,
                IPointerRoutedEventArgs::GetCurrentPoint,
                element.as_raw(),
                raw
            )
        })
        .ok()?;
        let mut device = 0i32;
        call!(point, IPointerPoint::GetPointerDeviceType, &mut device);
        let properties = out(|raw| call!(point, IPointerPoint::GetProperties, raw)).ok()?;
        let mut pressed = 0u8;
        call!(
            properties,
            IPointerPointProperties::GetIsLeftButtonPressed,
            &mut pressed
        );
        // PointerDeviceType::Mouse is 2.
        if device != 2 || pressed == 0 {
            return None;
        }
        let mut position = Point::default();
        call!(point, IPointerPoint::GetPosition, &mut position);
        Some((position, args))
    }
}

interface! {
    IAppWindow, IAppWindowVtbl: IInspectableVtbl {
        _unused_6_15: [*const c_void; 10],
        GetTitleBar: unsafe extern "system" fn(*mut IAppWindow, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IAppWindowStatics, IAppWindowStaticsVtbl: IInspectableVtbl {
        _unused_6_8: [*const c_void; 3],
        GetFromWindowId: unsafe extern "system" fn(*mut IAppWindowStatics, WindowId, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IAppWindowTitleBar, IAppWindowTitleBarVtbl: IInspectableVtbl {
        _unused_6: [*const c_void; 1],
        SetBackgroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_8: [*const c_void; 1],
        SetButtonBackgroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_10: [*const c_void; 1],
        SetButtonForegroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_12: [*const c_void; 1],
        SetButtonHoverBackgroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_14: [*const c_void; 1],
        SetButtonHoverForegroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_16: [*const c_void; 1],
        SetButtonInactiveBackgroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_18: [*const c_void; 1],
        SetButtonInactiveForegroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_20: [*const c_void; 1],
        SetButtonPressedBackgroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_22: [*const c_void; 1],
        SetButtonPressedForegroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_24: [*const c_void; 1],
        SetExtendsContentIntoTitleBar: unsafe extern "system" fn(*mut IAppWindowTitleBar, u8) -> HRESULT,
        _unused_26_31: [*const c_void; 6],
        SetInactiveBackgroundColor: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut c_void) -> HRESULT,
        _unused_33_35: [*const c_void; 3],
        GetRightInset: unsafe extern "system" fn(*mut IAppWindowTitleBar, *mut i32) -> HRESULT,
    }
}

interface! {
    IAppWindowTitleBar2, IAppWindowTitleBar2Vtbl: IInspectableVtbl {
        _unused_6: [*const c_void; 1],
        SetPreferredHeightOption: unsafe extern "system" fn(*mut IAppWindowTitleBar2, i32) -> HRESULT,
    }
}

interface! {
    IApplication, IApplicationVtbl: IInspectableVtbl {
        _unused_6: [*const c_void; 1],
        SetResources: unsafe extern "system" fn(*mut IApplication, *mut c_void) -> HRESULT,
    }
}

interface! {
    IBorder, IBorderVtbl: IInspectableVtbl {
        _unused_6_18: [*const c_void; 13],
        SetChild: unsafe extern "system" fn(*mut IBorder, *mut c_void) -> HRESULT,
    }
}

interface! {
    IBufferByteAccess, IBufferByteAccessVtbl: IUnknownVtbl {
        Buffer: unsafe extern "system" fn(*mut IBufferByteAccess, *mut *mut u8) -> HRESULT,
    }
}

interface! {
    IButtonBase, IButtonBaseVtbl: IInspectableVtbl {
        _unused_6_13: [*const c_void; 8],
        AddClick: unsafe extern "system" fn(*mut IButtonBase, *mut c_void, *mut i64) -> HRESULT,
    }
}

interface! {
    IClosable, IClosableVtbl: IInspectableVtbl {
        Close: unsafe extern "system" fn(*mut IClosable) -> HRESULT,
    }
}

interface! {
    ICompositionTargetStatics, ICompositionTargetStaticsVtbl: IInspectableVtbl {
        _unused_6_7: [*const c_void; 2],
        AddRendered: unsafe extern "system" fn(*mut ICompositionTargetStatics, *mut c_void, *mut i64) -> HRESULT,
        RemoveRendered: unsafe extern "system" fn(*mut ICompositionTargetStatics, i64) -> HRESULT,
    }
}

interface! {
    IContentDialog, IContentDialogVtbl: IInspectableVtbl {
        _unused_6_43: [*const c_void; 38],
        AddClosed: unsafe extern "system" fn(*mut IContentDialog, *mut c_void, *mut i64) -> HRESULT,
        RemoveClosed: unsafe extern "system" fn(*mut IContentDialog, i64) -> HRESULT,
        _unused_46_53: [*const c_void; 8],
        Hide: unsafe extern "system" fn(*mut IContentDialog) -> HRESULT,
        ShowAsync: unsafe extern "system" fn(*mut IContentDialog, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IContentDialogClosedEventArgs, IContentDialogClosedEventArgsVtbl: IInspectableVtbl {
        GetResult: unsafe extern "system" fn(*mut IContentDialogClosedEventArgs, *mut i32) -> HRESULT,
    }
}

interface! {
    #[cfg(feature = "edit")]
    IControl, IControlVtbl: IInspectableVtbl {
        _unused_6_28: [*const c_void; 23],
        SetIsEnabled: unsafe extern "system" fn(*mut IControl, bool) -> HRESULT,
    }
}

interface! {
    IDesktopSiteBridge, IDesktopSiteBridgeVtbl: IInspectableVtbl {
        _unused_6_12: [*const c_void; 7],
        MoveAndResize: unsafe extern "system" fn(*mut IDesktopSiteBridge, RectInt32) -> HRESULT,
        _unused_14_16: [*const c_void; 3],
        Show: unsafe extern "system" fn(*mut IDesktopSiteBridge) -> HRESULT,
    }
}

interface! {
    IDesktopWindowXamlSource, IDesktopWindowXamlSourceVtbl: IInspectableVtbl {
        _unused_6: [*const c_void; 1],
        SetContent: unsafe extern "system" fn(*mut IDesktopWindowXamlSource, *mut c_void) -> HRESULT,
        _unused_8_9: [*const c_void; 2],
        SetSystemBackdrop: unsafe extern "system" fn(*mut IDesktopWindowXamlSource, *mut c_void) -> HRESULT,
        GetSiteBridge: unsafe extern "system" fn(*mut IDesktopWindowXamlSource, *mut *mut c_void) -> HRESULT,
        _unused_12_16: [*const c_void; 5],
        Initialize: unsafe extern "system" fn(*mut IDesktopWindowXamlSource, WindowId) -> HRESULT,
    }
}

interface! {
    IDispatcherQueueController2, IDispatcherQueueController2Vtbl: IInspectableVtbl {
        ShutdownQueue: unsafe extern "system" fn(*mut IDispatcherQueueController2) -> HRESULT,
    }
}

interface! {
    IDispatcherQueueControllerStatics, IDispatcherQueueControllerStaticsVtbl: IInspectableVtbl {
        _unused_6: [*const c_void; 1],
        CreateOnCurrentThread: unsafe extern "system" fn(*mut IDispatcherQueueControllerStatics, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IFrameworkElement, IFrameworkElementVtbl: IInspectableVtbl {
        _unused_6_12: [*const c_void; 7],
        GetActualWidth: unsafe extern "system" fn(*mut IFrameworkElement, *mut f64) -> HRESULT,
        _unused_14_15: [*const c_void; 2],
        SetWidth: unsafe extern "system" fn(*mut IFrameworkElement, f64) -> HRESULT,
        _unused_17: [*const c_void; 1],
        SetHeight: unsafe extern "system" fn(*mut IFrameworkElement, f64) -> HRESULT,
        _unused_19_31: [*const c_void; 13],
        SetMargin: unsafe extern "system" fn(*mut IFrameworkElement, Thickness) -> HRESULT,
        GetName: unsafe extern "system" fn(*mut IFrameworkElement, *mut HSTRING) -> HRESULT,
        _unused_34_56: [*const c_void; 23],
        GetRequestedTheme: unsafe extern "system" fn(*mut IFrameworkElement, *mut i32) -> HRESULT,
        SetRequestedTheme: unsafe extern "system" fn(*mut IFrameworkElement, i32) -> HRESULT,
        _unused_59_66: [*const c_void; 8],
        AddSizeChanged: unsafe extern "system" fn(*mut IFrameworkElement, *mut c_void, *mut i64) -> HRESULT,
        _unused_68_76: [*const c_void; 9],
        FindName: unsafe extern "system" fn(*mut IFrameworkElement, HSTRING, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IImage, IImageVtbl: IInspectableVtbl {
        _unused_6: [*const c_void; 1],
        SetSource: unsafe extern "system" fn(*mut IImage, *mut c_void) -> HRESULT,
    }
}

interface! {
    IInputNonClientPointerSource, IInputNonClientPointerSourceVtbl: IInspectableVtbl {
        _unused_6_9: [*const c_void; 4],
        SetRegionRects: unsafe extern "system" fn(*mut IInputNonClientPointerSource, i32, u32, *const RectInt32) -> HRESULT,
    }
}

interface! {
    IInputNonClientPointerSourceStatics, IInputNonClientPointerSourceStaticsVtbl: IInspectableVtbl {
        GetForWindowId: unsafe extern "system" fn(*mut IInputNonClientPointerSourceStatics, WindowId, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    #[cfg(feature = "edit")]
    IKeyRoutedEventArgs, IKeyRoutedEventArgsVtbl: IInspectableVtbl {
        GetKey: unsafe extern "system" fn(*mut IKeyRoutedEventArgs, *mut i32) -> HRESULT,
        _unused_7_8: [*const c_void; 2],
        SetHandled: unsafe extern "system" fn(*mut IKeyRoutedEventArgs, u8) -> HRESULT,
    }
}

interface! {
    IMenuFlyoutItem, IMenuFlyoutItemVtbl: IInspectableVtbl {
        _unused_6_16: [*const c_void; 11],
        AddClick: unsafe extern "system" fn(*mut IMenuFlyoutItem, *mut c_void, *mut i64) -> HRESULT,
    }
}

interface! {
    IPointerPoint, IPointerPointVtbl: IInspectableVtbl {
        _unused_6_7: [*const c_void; 2],
        GetPointerDeviceType: unsafe extern "system" fn(*mut IPointerPoint, *mut i32) -> HRESULT,
        _unused_9: [*const c_void; 1],
        GetPosition: unsafe extern "system" fn(*mut IPointerPoint, *mut Point) -> HRESULT,
        GetProperties: unsafe extern "system" fn(*mut IPointerPoint, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IPointerPointProperties, IPointerPointPropertiesVtbl: IInspectableVtbl {
        _unused_6_12: [*const c_void; 7],
        GetIsLeftButtonPressed: unsafe extern "system" fn(*mut IPointerPointProperties, *mut u8) -> HRESULT,
    }
}

interface! {
    IPointerRoutedEventArgs, IPointerRoutedEventArgsVtbl: IInspectableVtbl {
        GetPointer: unsafe extern "system" fn(*mut IPointerRoutedEventArgs, *mut *mut c_void) -> HRESULT,
        GetKeyModifiers: unsafe extern "system" fn(*mut IPointerRoutedEventArgs, *mut u32) -> HRESULT,
        _unused_8: [*const c_void; 1],
        SetHandled: unsafe extern "system" fn(*mut IPointerRoutedEventArgs, u8) -> HRESULT,
        _unused_10: [*const c_void; 1],
        GetCurrentPoint: unsafe extern "system" fn(*mut IPointerRoutedEventArgs, *mut c_void, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IPropertyValueStatics, IPropertyValueStaticsVtbl: IInspectableVtbl {
        _unused_6_13: [*const c_void; 8],
        CreateSingle: unsafe extern "system" fn(*mut IPropertyValueStatics, f32, *mut *mut c_void) -> HRESULT,
        CreateDouble: unsafe extern "system" fn(*mut IPropertyValueStatics, f64, *mut *mut c_void) -> HRESULT,
        _unused_16: [*const c_void; 1],
        CreateBoolean: unsafe extern "system" fn(*mut IPropertyValueStatics, bool, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IScrollViewer, IScrollViewerVtbl: IInspectableVtbl {
        _unused_6_39: [*const c_void; 34],
        GetHorizontalOffset: unsafe extern "system" fn(*mut IScrollViewer, *mut f64) -> HRESULT,
        GetViewportWidth: unsafe extern "system" fn(*mut IScrollViewer, *mut f64) -> HRESULT,
        GetScrollableWidth: unsafe extern "system" fn(*mut IScrollViewer, *mut f64) -> HRESULT,
        _unused_43_44: [*const c_void; 2],
        GetVerticalOffset: unsafe extern "system" fn(*mut IScrollViewer, *mut f64) -> HRESULT,
        _unused_46: [*const c_void; 1],
        GetScrollableHeight: unsafe extern "system" fn(*mut IScrollViewer, *mut f64) -> HRESULT,
        _unused_48_53: [*const c_void; 6],
        GetZoomFactor: unsafe extern "system" fn(*mut IScrollViewer, *mut f32) -> HRESULT,
        _unused_55_73: [*const c_void; 19],
        AddViewChanged: unsafe extern "system" fn(*mut IScrollViewer, *mut c_void, *mut i64) -> HRESULT,
        _unused_75_83: [*const c_void; 9],
        ChangeView: unsafe extern "system" fn(*mut IScrollViewer, *mut c_void, *mut c_void, *mut c_void, u8, *mut u8) -> HRESULT,
    }
}

interface! {
    IScrollViewerViewChangedEventArgs, IScrollViewerViewChangedEventArgsVtbl: IInspectableVtbl {
        GetIsIntermediate: unsafe extern "system" fn(*mut IScrollViewerViewChangedEventArgs, *mut u8) -> HRESULT,
    }
}

interface! {
    ITextBlock, ITextBlockVtbl: IInspectableVtbl {
        _unused_6_26: [*const c_void; 21],
        SetText: unsafe extern "system" fn(*mut ITextBlock, *mut c_void) -> HRESULT,
    }
}

interface! {
    IToggleButton, IToggleButtonVtbl: IInspectableVtbl {
        GetIsChecked: unsafe extern "system" fn(*mut IToggleButton, *mut *mut c_void) -> HRESULT,
        SetIsChecked: unsafe extern "system" fn(*mut IToggleButton, *mut c_void) -> HRESULT,
    }
}

interface! {
    IUIElement, IUIElementVtbl: IInspectableVtbl {
        _unused_6_9: [*const c_void; 4],
        SetOpacity: unsafe extern "system" fn(*mut IUIElement, f64) -> HRESULT,
        _unused_11_22: [*const c_void; 12],
        GetVisibility: unsafe extern "system" fn(*mut IUIElement, *mut i32) -> HRESULT,
        SetVisibility: unsafe extern "system" fn(*mut IUIElement, i32) -> HRESULT,
        _unused_25_108: [*const c_void; 84],
        GetXamlRoot: unsafe extern "system" fn(*mut IUIElement, *mut *mut c_void) -> HRESULT,
        SetXamlRoot: unsafe extern "system" fn(*mut IUIElement, *mut c_void) -> HRESULT,
        _unused_111_131: [*const c_void; 21],
        AddKeyDown: unsafe extern "system" fn(*mut IUIElement, *mut c_void, *mut i64) -> HRESULT,
        _unused_133_151: [*const c_void; 19],
        AddPointerPressed: unsafe extern "system" fn(*mut IUIElement, *mut c_void, *mut i64) -> HRESULT,
        _unused_153: [*const c_void; 1],
        AddPointerMoved: unsafe extern "system" fn(*mut IUIElement, *mut c_void, *mut i64) -> HRESULT,
        _unused_155: [*const c_void; 1],
        AddPointerReleased: unsafe extern "system" fn(*mut IUIElement, *mut c_void, *mut i64) -> HRESULT,
        _unused_157_161: [*const c_void; 5],
        AddPointerCaptureLost: unsafe extern "system" fn(*mut IUIElement, *mut c_void, *mut i64) -> HRESULT,
        _unused_163_211: [*const c_void; 49],
        CapturePointer: unsafe extern "system" fn(*mut IUIElement, *mut c_void, *mut u8) -> HRESULT,
        _unused_213_225: [*const c_void; 13],
        Focus: unsafe extern "system" fn(*mut IUIElement, i32, *mut u8) -> HRESULT,
    }
}

interface! {
    IWindowsXamlManagerStatics, IWindowsXamlManagerStaticsVtbl: IInspectableVtbl {
        InitializeForCurrentThread: unsafe extern "system" fn(*mut IWindowsXamlManagerStatics, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IWriteableBitmap, IWriteableBitmapVtbl: IInspectableVtbl {
        GetPixelBuffer: unsafe extern "system" fn(*mut IWriteableBitmap, *mut *mut c_void) -> HRESULT,
        Invalidate: unsafe extern "system" fn(*mut IWriteableBitmap) -> HRESULT,
    }
}

interface! {
    IWriteableBitmapFactory, IWriteableBitmapFactoryVtbl: IInspectableVtbl {
        CreateInstanceWithDimensions: unsafe extern "system" fn(*mut IWriteableBitmapFactory, i32, i32, *mut *mut c_void) -> HRESULT,
    }
}

interface! {
    IXamlReaderStatics, IXamlReaderStaticsVtbl: IInspectableVtbl {
        Load: unsafe extern "system" fn(*mut IXamlReaderStatics, HSTRING, *mut *mut c_void) -> HRESULT,
    }
}
