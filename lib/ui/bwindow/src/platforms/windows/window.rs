/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::env;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

#[cfg(feature = "progress_bar")]
use super::event_loop::TASKBAR_BUTTON_CREATED;
use super::event_loop::{APP_ID, send_event, system_theme};
#[cfg(feature = "file_drop")]
use super::file_drop::handle_file_drop;
use super::headers::*;
use super::interaction::{Interaction, release_pointer_lock};
#[cfg(feature = "progress_bar")]
use super::progress_bar::ProgressBar;
#[cfg(feature = "remember_window_state")]
use super::window_state::{restore_window_state, save_window_state};
#[cfg(feature = "progress_bar")]
use crate::WindowsProgressBarState;
use crate::{
    CloseRequest, CursorIcon, LogicalPoint, LogicalSize, PointerLockError, Theme, WindowBuilder,
    WindowEvent,
};

pub(super) struct WindowData {
    host: std::rc::Rc<crate::content::ContentHost>,
    pub(crate) window_id: crate::WindowId,
    pub(super) hwnd: HWND,
    pub(super) dpi: u32,
    pub(super) min_size: Option<LogicalSize>,
    pub(super) background_color: Option<u32>,
    theme: Option<Theme>,
    interaction: Interaction,
    #[cfg(feature = "remember_window_state")]
    pub(super) remember_window_state: bool,
    #[cfg(feature = "progress_bar")]
    progress_bar: ProgressBar,
}

thread_local! {
    static WINDOW_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) struct PlatformWindow(pub(super) Box<WindowData>);

fn logical_to_physical(value: f32, dpi: u32) -> i32 {
    (value * dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32).round() as i32
}

fn physical_to_logical(value: i32, dpi: u32) -> f32 {
    value as f32 * USER_DEFAULT_SCREEN_DPI as f32 / dpi.max(1) as f32
}

fn calculate_window_rect(
    builder: &WindowBuilder,
    monitor_rect: &RECT,
    style: u32,
    dpi: u32,
) -> (RECT, bool) {
    let mut position_set = false;
    let mut x = 0;
    let mut y = 0;
    let mut width = logical_to_physical(builder.size.width, dpi);
    let mut height = logical_to_physical(builder.size.height, dpi);
    if let Some(position) = builder.position {
        position_set = true;
        x = monitor_rect.left + logical_to_physical(position.x, dpi);
        y = monitor_rect.top + logical_to_physical(position.y, dpi);
    }
    if builder.should_fullscreen {
        position_set = true;
        x = monitor_rect.left;
        y = monitor_rect.top;
        width = monitor_rect.right - monitor_rect.left;
        height = monitor_rect.bottom - monitor_rect.top;
    } else if builder.should_center {
        position_set = true;
        x = monitor_rect.left + ((monitor_rect.right - monitor_rect.left) - width) / 2;
        y = monitor_rect.top + ((monitor_rect.bottom - monitor_rect.top) - height) / 2;
    } else if !position_set && builder.monitor.is_some() {
        position_set = true;
        x = monitor_rect.left + logical_to_physical(64.0, dpi);
        y = monitor_rect.top + logical_to_physical(64.0, dpi);
    }
    let mut rect = RECT {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    };
    // SAFETY: rect is valid and style is a supported window style.
    unsafe { AdjustWindowRectExForDpi(&mut rect, style, FALSE, 0, dpi) };
    (rect, position_set)
}

impl WindowData {
    fn client_rect(&self, size: LogicalSize) -> RECT {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: logical_to_physical(size.width, self.dpi),
            bottom: logical_to_physical(size.height, self.dpi),
        };
        unsafe {
            let style = GetWindowLongPtrW(self.hwnd, GWL_STYLE) as u32;
            AdjustWindowRectExForDpi(&mut rect, style, FALSE, 0, self.dpi);
        }
        rect
    }
}

impl PlatformWindow {
    pub(crate) const fn id(&self) -> crate::WindowId {
        self.0.window_id
    }

    pub(crate) fn new(
        builder: &WindowBuilder,
        host: std::rc::Rc<crate::content::ContentHost>,
    ) -> Self {
        let initial_dpi = unsafe { GetDpiForSystem() };
        let mut window_data = Box::new(WindowData {
            host: host.clone(),
            window_id: builder.window_id,
            hwnd: null_mut(),
            dpi: initial_dpi,
            min_size: builder.min_size,
            background_color: builder.background_color,
            theme: builder.theme,
            interaction: Interaction::default(),
            #[cfg(feature = "remember_window_state")]
            remember_window_state: builder.remember_window_state,
            #[cfg(feature = "progress_bar")]
            progress_bar: ProgressBar::default(),
        });

        // Check if window class is already registered
        let instance = unsafe { GetModuleHandleW(null()) };
        let class_name = unsafe {
            if let Some(ref app_id) = APP_ID {
                format!(
                    "bwindow-{}.{}.{}",
                    app_id.qualifier, app_id.organization, app_id.application
                )
            } else {
                "bwindow".to_string()
            }
        };
        let class_name = class_name.to_wide_string();
        unsafe {
            let mut wndclass = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                ..Default::default()
            };
            if GetClassInfoExW(instance, class_name.as_ptr(), &mut wndclass as *mut _) != TRUE {
                // Get executable icons
                let executable_path = env::current_exe()
                    .expect("Can't get current exe path")
                    .display()
                    .to_string()
                    .to_wide_string();
                let mut large_icon = HICON::default();
                let mut small_icon = HICON::default();
                ExtractIconExW(
                    executable_path.as_ptr(),
                    0,
                    &mut large_icon,
                    &mut small_icon,
                    1,
                );

                // Register window class
                let wndclass = WNDCLASSEXW {
                    cbSize: size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(window_proc),
                    hInstance: instance,
                    hIcon: large_icon,
                    hbrBackground: GetSysColorBrush(COLOR_WINDOW) as usize,
                    lpszClassName: class_name.as_ptr(),
                    hIconSm: small_icon,
                    ..Default::default()
                };
                RegisterClassExW(&wndclass);
            }
        }

        // Create window
        let (hwnd, restored_window_state) = unsafe {
            let style = if builder.should_fullscreen {
                WS_POPUP
            } else if builder.resizable {
                WS_OVERLAPPEDWINDOW
            } else {
                WS_OVERLAPPEDWINDOW & !WS_THICKFRAME & !WS_MAXIMIZEBOX
            };

            let monitor_rect = if let Some(monitor) = builder.monitor {
                monitor.rect()
            } else {
                RECT {
                    left: 0,
                    top: 0,
                    right: GetSystemMetrics(SM_CXSCREEN),
                    bottom: GetSystemMetrics(SM_CYSCREEN),
                }
            };
            let (rect, position_set) =
                calculate_window_rect(builder, &monitor_rect, style, initial_dpi);

            let title = builder.title.to_wide_string();
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                style,
                if position_set {
                    rect.left
                } else {
                    CW_USEDEFAULT
                },
                if position_set {
                    rect.top
                } else {
                    CW_USEDEFAULT
                },
                rect.right - rect.left,
                rect.bottom - rect.top,
                null_mut(),
                null_mut(),
                instance,
                window_data.as_mut() as *mut WindowData as LPARAM,
            );
            window_data.hwnd = hwnd;
            host.set_handle(crate::NativeWindowHandle::Win32(hwnd));
            // Renderers draw into the client area, which receives the interaction.
            host.set_content(crate::NativeContentHandle::Win32(hwnd));
            #[cfg(feature = "file_drop")]
            if builder.allow_file_drop {
                DragAcceptFiles(hwnd, TRUE);
            }
            set_titlebar_theme(hwnd, builder.theme.unwrap_or_else(system_theme));

            #[cfg(feature = "remember_window_state")]
            let restored_window_state = builder.remember_window_state && restore_window_state(hwnd);
            #[cfg(not(feature = "remember_window_state"))]
            let restored_window_state = false;
            let window_dpi = GetDpiForWindow(hwnd);
            let dpi = if window_dpi == 0 {
                initial_dpi
            } else {
                window_dpi
            };
            window_data.dpi = dpi;
            host.scale_factor_changed(dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32);
            if !restored_window_state && dpi != initial_dpi {
                let (rect, position_set) =
                    calculate_window_rect(builder, &monitor_rect, style, dpi);
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE | if position_set { 0 } else { SWP_NOMOVE },
                );
            }
            (hwnd, restored_window_state)
        };

        unsafe {
            if !restored_window_state {
                ShowWindow(hwnd, SW_SHOWDEFAULT);
            }
            UpdateWindow(hwnd);
        }

        PlatformWindow(window_data)
    }
}

impl crate::WindowInterface for PlatformWindow {
    fn close(&mut self) {
        #[cfg(feature = "remember_window_state")]
        if self.0.remember_window_state {
            save_window_state(self.0.hwnd);
        }
        unsafe { DestroyWindow(self.0.hwnd) };
    }

    fn set_title(&mut self, title: impl AsRef<str>) {
        let title = title.as_ref().to_wide_string();
        unsafe { SetWindowTextW(self.0.hwnd, title.as_ptr()) };
    }

    fn position(&self) -> LogicalPoint {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(self.0.hwnd, &mut rect) };
        LogicalPoint::new(
            physical_to_logical(rect.left, self.0.dpi),
            physical_to_logical(rect.top, self.0.dpi),
        )
    }

    fn size(&self) -> LogicalSize {
        let mut rect = RECT::default();
        unsafe { GetClientRect(self.0.hwnd, &mut rect) };
        LogicalSize::new(
            physical_to_logical(rect.right - rect.left, self.0.dpi),
            physical_to_logical(rect.bottom - rect.top, self.0.dpi),
        )
    }

    fn set_position(&mut self, point: LogicalPoint) {
        unsafe {
            SetWindowPos(
                self.0.hwnd,
                null_mut(),
                logical_to_physical(point.x, self.0.dpi),
                logical_to_physical(point.y, self.0.dpi),
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    fn set_size(&mut self, size: LogicalSize) {
        let rect = self.0.client_rect(size);
        unsafe {
            SetWindowPos(
                self.0.hwnd,
                null_mut(),
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOREPOSITION | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE,
            )
        };
    }

    fn set_min_size(&mut self, min_size: LogicalSize) {
        self.0.min_size = Some(min_size);
    }

    fn set_resizable(&mut self, resizable: bool) {
        unsafe {
            let style = GetWindowLong(self.0.hwnd, GWL_STYLE) as u32;
            SetWindowLong(
                self.0.hwnd,
                GWL_STYLE,
                if resizable {
                    style & !WS_THICKFRAME
                } else {
                    style | WS_THICKFRAME
                } as isize,
            );
        }
    }

    fn set_theme(&mut self, theme: Theme) {
        self.0.theme = Some(theme);
        let host = self.0.host.clone();
        unsafe { set_titlebar_theme(self.0.hwnd, theme) };
        host.theme_changed(theme);
    }

    fn set_background_color(&mut self, color: u32) {
        self.0.background_color = Some(color);
        unsafe { InvalidateRect(self.0.hwnd, null_mut(), TRUE) };
    }

    fn set_cursor(&mut self, cursor: CursorIcon) {
        self.0.interaction.set_cursor(self.0.hwnd, cursor);
    }

    fn request_pointer_lock(&mut self) -> Result<(), PointerLockError> {
        self.0.interaction.lock_pointer(self.0.hwnd)
    }

    fn exit_pointer_lock(&mut self) {
        release_pointer_lock();
    }

    #[cfg(feature = "progress_bar")]
    fn windows_set_progress_bar(&mut self, progress: Option<f32>, state: WindowsProgressBarState) {
        self.0.progress_bar.set(self.0.hwnd, progress, state);
    }
}

unsafe fn set_titlebar_theme(hwnd: HWND, theme: Theme) {
    let enabled: BOOL = (theme == Theme::Dark).into();
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &enabled as *const _ as *const _,
            size_of::<BOOL>() as u32,
        );
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let _self = unsafe {
        let ptr = if msg == WM_NCCREATE {
            let create = &*(l_param as *const CREATESTRUCTW);
            let ptr = create.lpCreateParams.cast::<WindowData>();
            SetWindowLong(hwnd, GWL_USERDATA, ptr as isize);
            ptr
        } else {
            GetWindowLong(hwnd, GWL_USERDATA) as *mut WindowData
        };
        let Some(window_data) = ptr.as_mut() else {
            return DefWindowProcW(hwnd, msg, w_param, l_param);
        };
        window_data
    };
    if let Some(result) = unsafe {
        _self
            .interaction
            .handle(hwnd, &_self.host, _self.dpi, msg, w_param, l_param)
    } {
        return result;
    }
    match msg {
        super::frame::WM_ANIMATION_FRAME => {
            let host = _self.host.clone();
            host.animation_frame();
            0
        }
        WM_PAINT => {
            // Validate the update region first, so content that requests another frame
            // during its paint pass gets exactly one more WM_PAINT and never a loop.
            let mut paint = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut paint) };
            unsafe { EndPaint(hwnd, &paint) };
            // Painting can run application code and destroy this window.
            // End the raw WindowData borrow before entering that callback.
            let host = _self.host.clone();
            if !host.paint() {
                host.event_sender().send(WindowEvent::RedrawRequested);
            }
            0
        }
        // WM_SETTINGCHANGE and WM_THEMECHANGED, including the application's color mode.
        0x001A | 0x031A => {
            let theme = _self.theme.unwrap_or_else(system_theme);
            let host = _self.host.clone();
            unsafe { set_titlebar_theme(hwnd, theme) };
            host.theme_changed(theme);
            0
        }
        #[cfg(feature = "progress_bar")]
        message if message == unsafe { TASKBAR_BUTTON_CREATED } => {
            _self.progress_bar.button_created(hwnd);
            0
        }
        // Keyboard input goes to the native view filling the window.
        WM_SETFOCUS if let Some(view) = _self.host.native_view() => {
            unsafe { SetFocus(view) };
            0
        }
        WM_CREATE => {
            WINDOW_COUNT.with(|count| count.set(count.get() + 1));
            send_event(crate::Event::Window(_self.window_id, WindowEvent::Create));
            0
        }
        // Content painting the client area draws the background itself, erasing would flicker.
        WM_ERASEBKGND if _self.host.has_paint() => 1,
        WM_ERASEBKGND => {
            if let Some(color) = _self.background_color {
                let hdc = w_param as HDC;
                let mut client_rect = RECT::default();
                unsafe { GetClientRect(hwnd, &mut client_rect) };
                let brush = unsafe {
                    CreateSolidBrush(
                        ((color & 0xFF) << 16) | (color & 0xFF00) | ((color >> 16) & 0xFF),
                    )
                };
                unsafe { FillRect(hdc, &client_rect, brush) };
                unsafe { DeleteObject(brush) };
                1
            } else {
                0
            }
        }
        WM_MOVE => {
            let x = l_param as u16 as i16 as i32;
            let y = (l_param >> 16) as u16 as i16 as i32;
            send_event(crate::Event::Window(
                _self.window_id,
                WindowEvent::Move(LogicalPoint::new(
                    physical_to_logical(x, _self.dpi),
                    physical_to_logical(y, _self.dpi),
                )),
            ));
            0
        }
        WM_SIZE => {
            let width = (l_param as u16) as i32;
            let height = ((l_param >> 16) as u16) as i32;
            let host = _self.host.clone();
            let id = _self.window_id;
            let dpi = _self.dpi;
            // Native content resizing may reenter application code and drop the window.
            host.resize(width as u32, height as u32);
            if host.is_closed() {
                return 0;
            }
            send_event(crate::Event::Window(
                id,
                WindowEvent::Resize(LogicalSize::new(
                    physical_to_logical(width, dpi),
                    physical_to_logical(height, dpi),
                )),
            ));
            0
        }
        WM_DPICHANGED => {
            _self.dpi = (w_param >> 16) as u32;
            let host = _self.host.clone();
            let scale = _self.dpi as f32 / USER_DEFAULT_SCREEN_DPI as f32;
            let window_rect = unsafe { &*(l_param as *const RECT) };
            unsafe {
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    window_rect.left,
                    window_rect.top,
                    window_rect.right - window_rect.left,
                    window_rect.bottom - window_rect.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            host.scale_factor_changed(scale);
            if !host.is_closed() {
                send_event(crate::Event::Window(
                    host.id(),
                    WindowEvent::ScaleFactorChanged(scale),
                ));
                host.request_redraw();
            }
            0
        }
        WM_GETMINMAXINFO => {
            if let Some(min_size) = _self.min_size {
                let rect = _self.client_rect(min_size);
                unsafe {
                    let minmax_info: *mut MINMAXINFO = l_param as *mut MINMAXINFO;
                    (*minmax_info).ptMinTrackSize.x = rect.right - rect.left;
                    (*minmax_info).ptMinTrackSize.y = rect.bottom - rect.top;
                }
            }
            0
        }
        #[cfg(feature = "file_drop")]
        WM_DROPFILES => {
            unsafe { handle_file_drop(_self.window_id, w_param as HDROP) };
            0
        }
        WM_CLOSE => {
            let request = CloseRequest::new();
            #[cfg(feature = "remember_window_state")]
            let remember = _self.remember_window_state;
            let id = _self.window_id;
            crate::dispatch::send_then(
                crate::Event::Window(id, WindowEvent::CloseRequested(request.clone())),
                move || {
                    // Another queued close may already have destroyed this native window.
                    let data = unsafe { GetWindowLong(hwnd, GWL_USERDATA) as *const WindowData };
                    if data.is_null()
                        || unsafe { (*data).window_id } != id
                        || request.default_prevented()
                    {
                        return;
                    }
                    #[cfg(feature = "remember_window_state")]
                    if remember {
                        save_window_state(hwnd);
                    }
                    unsafe { (*data).host.close() };
                    unsafe { DestroyWindow(hwnd) };
                },
            );
            0
        }
        WM_DESTROY => {
            _self.host.close();
            WINDOW_COUNT.with(|count| {
                count.set(count.get() - 1);
                if count.get() == 0 {
                    unsafe { PostQuitMessage(0) };
                }
            });
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) },
    }
}

/// Fill the client area of the window with its child window and focus it.
pub(crate) unsafe fn embed_native_view(
    window: crate::NativeWindowHandle,
    _content: Option<crate::NativeContentHandle>,
    view: crate::NativeViewHandle,
) {
    let (crate::NativeWindowHandle::Win32(window), crate::NativeViewHandle::Win32(view)) =
        (window, view)
    else {
        panic!("invalid native view handle");
    };
    let mut rect = RECT::default();
    unsafe { GetClientRect(window, &mut rect) };
    resize_native_view(view, rect.right as u32, rect.bottom as u32);
    unsafe { ShowWindow(view, SW_SHOW) };
    if unsafe { GetActiveWindow() } == window {
        unsafe { SetFocus(view) };
    }
}

/// Size the native view to the client area of its window.
pub(crate) fn resize_native_view(view: HWND, width: u32, height: u32) {
    unsafe {
        SetWindowPos(
            view,
            null_mut(),
            0,
            0,
            width as i32,
            height as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

pub(crate) fn config_dir() -> PathBuf {
    let project_dirs = unsafe {
        if let Some(ref app_id) = APP_ID {
            directories::ProjectDirs::from(
                &app_id.qualifier,
                &app_id.organization,
                &app_id.application,
            )
        } else {
            directories::ProjectDirs::from_path(PathBuf::from(
                env::current_exe()
                    .expect("Can't get current process name")
                    .file_stem()
                    .expect("Can't get current process name"),
            ))
        }
    }
    .expect("Can't get dirs");
    project_dirs.config_dir()
}

pub(super) fn is_app_window(window: HWND) -> bool {
    !window.is_null() && unsafe { GetWindowLong(window, -4) } == window_proc as *const () as isize
}

// Activation runs on the UI thread after initialization, even when another process
// launched while the first window was still being built.
pub(super) fn activate() {
    unsafe extern "system" fn activate_window(window: HWND, _: LPARAM) -> BOOL {
        if is_app_window(window) && unsafe { IsWindowVisible(window) } != FALSE {
            if unsafe { IsIconic(window) } != FALSE {
                unsafe { ShowWindow(window, SW_RESTORE) };
            }
            unsafe { SetForegroundWindow(window) };
            FALSE
        } else {
            TRUE
        }
    }
    unsafe { EnumThreadWindows(GetCurrentThreadId(), activate_window, 0) };
}

#[cfg(test)]
mod dpi_tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn redraw_requests_coalesce_until_paint_and_allow_input_first() {
        use crate::WindowInterface;

        let builder = WindowBuilder::new().title("Redraw scheduling test");
        let host = crate::content::ContentHost::new(builder.window_id, None, false);
        let mut window = PlatformWindow::new(&builder, host.clone());
        let events = Rc::new(RefCell::new(Vec::new()));
        let output = events.clone();
        crate::dispatch::start(move |event| match event {
            crate::Event::Window(_, WindowEvent::RedrawRequested) => {
                output.borrow_mut().push("paint");
            }
            crate::Event::Window(_, WindowEvent::KeyDown(_)) => {
                output.borrow_mut().push("key");
            }
            _ => {}
        });
        events.borrow_mut().clear();

        host.request_redraw();
        host.request_redraw();
        assert!(events.borrow().is_empty(), "redraw must wait for WM_PAINT");
        // Input delivered before the pending paint must reach the application first.
        unsafe { window_proc(window.0.hwnd, 0x0100, 0x41, 0x001e0001) };
        unsafe { UpdateWindow(window.0.hwnd) };
        assert_eq!(*events.borrow(), ["key", "paint"]);
        unsafe { UpdateWindow(window.0.hwnd) };
        assert_eq!(events.borrow().len(), 2, "redraws must coalesce");

        window.close();
        crate::dispatch::shutdown(&crate::mailbox::Mailbox::new());
    }

    #[test]
    fn dpi_conversion_preserves_fractional_logical_coordinates() {
        assert!((physical_to_logical(101, 144) - 67.333_336).abs() < 0.000_01);
        assert_eq!(physical_to_logical(-3, 192), -1.5);
        assert_eq!(logical_to_physical(1.5, 192), 3);
        assert_eq!(logical_to_physical(-1.5, 192), -3);
        assert_eq!(logical_to_physical(1.5, 144), 2);
    }
}
