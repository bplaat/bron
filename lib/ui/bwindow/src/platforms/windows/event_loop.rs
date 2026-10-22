/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;
use std::mem::{self, size_of};
use std::process::exit;
use std::ptr::{null, null_mut};
use std::sync::Arc;

use super::headers::*;
use crate::mailbox::{Mailbox, Message};
use crate::{AppId, EventLoopBuilder, LogicalPoint, LogicalSize, NativeEvent as Event, Theme};

pub(super) static mut APP_ID: Option<AppId> = None;
#[cfg(feature = "progress_bar")]
pub(super) static mut TASKBAR_BUTTON_CREATED: u32 = 0;

// MARK: EventLoop
pub(crate) struct PlatformEventLoop {
    theme: Theme,
    mailbox: Arc<Mailbox>,
    message_window: HWND,
    #[cfg(feature = "menu")]
    _accelerators: super::menu::Accelerators,
}

impl PlatformEventLoop {
    pub(crate) fn new(builder: EventLoopBuilder) -> Self {
        unsafe {
            // Ensure single instance
            if let Some(app_id) = builder.app_id {
                if builder.single_instance {
                    let mutex_name = format!(
                        "bwindow-{}.{}.{}",
                        app_id.qualifier, app_id.organization, app_id.application
                    );
                    let mutex_name = mutex_name.to_wide_string();
                    CreateMutexW(null_mut(), TRUE, mutex_name.as_ptr());
                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        let hwnd = FindWindowW(mutex_name.as_ptr(), null());
                        if !hwnd.is_null() {
                            ShowWindow(hwnd, SW_RESTORE);
                            SetForegroundWindow(hwnd);
                        }
                        exit(0);
                    }
                }
                APP_ID = Some(app_id);
            }

            // Initialize OLE (and COM as a single-threaded apartment). OLE
            // initialization is required for RegisterDragDrop to succeed.
            #[cfg(feature = "file_drop")]
            OleInitialize(null_mut());

            // Explorer creates the taskbar button asynchronously. Shell APIs must
            // not be used until the window receives this registered message.
            #[cfg(feature = "progress_bar")]
            {
                TASKBAR_BUTTON_CREATED =
                    RegisterWindowMessageW(wide!("TaskbarButtonCreated").as_ptr());
            }

            enable_high_dpi_awareness();

            let mailbox = Arc::new(Mailbox::new());
            let instance = GetModuleHandleW(null());
            let class = wide!("bwindow-event-loop");
            let definition = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(message_window_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            RegisterClassExW(&definition);
            let message_window = CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                0,
                0,
                0,
                0,
                0,
                -3isize as HWND,
                null_mut(),
                instance,
                Arc::as_ptr(&mailbox) as LPARAM,
            );
            assert!(
                !message_window.is_null(),
                "could not create event-loop message window"
            );
            Self {
                theme: system_theme(),
                mailbox,
                message_window,
                #[cfg(feature = "menu")]
                _accelerators: super::menu::Accelerators::new(
                    message_window,
                    builder.menu.as_ref(),
                ),
            }
        }
    }
}

// Enable PerMonitorV2 when available without requiring Windows 10 1703
unsafe fn enable_high_dpi_awareness() {
    type SetProcessDpiAwarenessContext = unsafe extern "system" fn(isize) -> BOOL;

    let user32 = unsafe { GetModuleHandleW(wide!("user32.dll").as_ptr()) };
    if !user32.is_null() {
        let proc = unsafe { GetProcAddress(user32, c"SetProcessDpiAwarenessContext".as_ptr()) };
        if !proc.is_null() {
            // SAFETY: proc points to SetProcessDpiAwarenessContext with this signature.
            let set_context: SetProcessDpiAwarenessContext = unsafe { mem::transmute(proc) };
            if unsafe { set_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) } != FALSE {
                return;
            }
        }
    }

    // Fall back to PerMonitor on older Windows versions
    unsafe {
        SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
    }
}

// MARK: Theme
pub(super) fn system_theme() -> Theme {
    let mut apps_use_light_theme = 1u32;
    let mut size = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
            wide!("AppsUseLightTheme").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut apps_use_light_theme as *mut _ as *mut c_void,
            &mut size,
        )
    };
    if status == ERROR_SUCCESS && apps_use_light_theme == 0 {
        Theme::Dark
    } else {
        Theme::Light
    }
}

impl crate::EventLoopInterface for PlatformEventLoop {
    fn theme(&self) -> Theme {
        self.theme
    }

    fn primary_monitor(&self) -> PlatformMonitor {
        let hmonitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        PlatformMonitor::new(hmonitor)
    }

    fn available_monitors(&self) -> Vec<PlatformMonitor> {
        unsafe extern "system" fn monitor_enum_proc(
            hmonitor: HMONITOR,
            _hdc_monitor: HDC,
            _lprc_monitor: *const RECT,
            data: LPARAM,
        ) -> BOOL {
            // EnumDisplayMonitors invokes this callback synchronously.
            let monitors = unsafe { &mut *(data as *mut Vec<PlatformMonitor>) };
            monitors.push(PlatformMonitor::new(hmonitor));
            true.into()
        }
        let mut monitors = Vec::new();
        unsafe {
            EnumDisplayMonitors(
                null_mut(),
                null_mut(),
                monitor_enum_proc,
                &mut monitors as *mut Vec<PlatformMonitor> as LPARAM,
            );
        }
        monitors
    }

    fn run(self, event_handler: impl FnMut(Event) + 'static) -> i32 {
        crate::dispatch::start(event_handler);

        // Start message loop
        unsafe {
            let mut msg = mem::zeroed();
            loop {
                let result = GetMessageW(&mut msg, null_mut(), 0, 0);
                assert!(result >= 0, "GetMessageW failed");
                if result == 0 {
                    return msg.wParam as i32;
                }
                #[cfg(feature = "menu")]
                if super::menu::translate(&mut msg) {
                    continue;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn create_proxy(&self) -> PlatformEventLoopProxy {
        PlatformEventLoopProxy {
            mailbox: self.mailbox.clone(),
            hwnd: self.message_window as usize,
        }
    }
}

pub(crate) use crate::dispatch::send as send_event;

impl Drop for PlatformEventLoop {
    fn drop(&mut self) {
        crate::dispatch::shutdown(&self.mailbox);
        unsafe { DestroyWindow(self.message_window) };
        #[cfg(feature = "file_drop")]
        unsafe {
            OleUninitialize()
        };
    }
}

// MARK: EventLoopProxy
const WM_WAKE: u32 = WM_USER + 1;

unsafe extern "system" fn message_window_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = unsafe { &*(l as *const CREATESTRUCTW) };
        unsafe { SetWindowLong(hwnd, GWL_USERDATA, create.lpCreateParams as isize) };
    }
    #[cfg(feature = "menu")]
    if msg == 0x0111 && w >> 16 == 1 {
        // Defer application callbacks until synchronous WebView2 input handling returns.
        unsafe { PostMessageW(hwnd, WM_USER + 2, w & 0xffff, 0) };
        return 0;
    }
    #[cfg(feature = "menu")]
    if msg == WM_USER + 2 {
        super::menu::activate(w);
        return 0;
    }
    if msg == WM_WAKE {
        let mailbox = unsafe { &*(GetWindowLong(hwnd, GWL_USERDATA) as *const Mailbox) };
        while let Some(message) = mailbox.pop() {
            match message {
                Message::User(value) => send_event(Event::UserEvent(value)),
                Message::Exit => {
                    unsafe { PostQuitMessage(0) };
                    break;
                }
            }
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

pub(crate) struct PlatformEventLoopProxy {
    mailbox: Arc<Mailbox>,
    hwnd: usize,
}

impl PlatformEventLoopProxy {
    fn send(&self, message: Message) -> bool {
        self.mailbox.send(message, || unsafe {
            PostMessageW(self.hwnd as HWND, WM_WAKE, 0, 0) != FALSE
        })
    }
}

impl crate::EventLoopProxyInterface for PlatformEventLoopProxy {
    fn send_user_event(&self, data: Box<dyn std::any::Any + Send>) -> bool {
        self.send(Message::User(data))
    }

    fn exit(&self) -> bool {
        self.send(Message::Exit)
    }
}

// MARK: Monitor
pub(crate) struct PlatformMonitor {
    hmonitor: HMONITOR,
    info: MONITORINFOEXW,
}

impl PlatformMonitor {
    pub(crate) fn new(hmonitor: HMONITOR) -> Self {
        let mut info = MONITORINFOEXW {
            cbSize: size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        };
        unsafe {
            GetMonitorInfoW(hmonitor, &mut info as *mut _ as *mut _);
        }
        Self { hmonitor, info }
    }

    pub(crate) fn rect(&self) -> RECT {
        self.info.rcMonitor.clone()
    }
}

impl crate::MonitorInterface for PlatformMonitor {
    fn name(&self) -> String {
        let len = self
            .info
            .szDevice
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(self.info.szDevice.len());
        String::from_utf16_lossy(&self.info.szDevice[..len])
    }

    fn position(&self) -> LogicalPoint {
        let scale = self.scale_factor();
        LogicalPoint::new(
            self.info.rcMonitor.left as f32 / scale,
            self.info.rcMonitor.top as f32 / scale,
        )
    }

    fn size(&self) -> LogicalSize {
        let scale = self.scale_factor();
        LogicalSize::new(
            (self.info.rcMonitor.right - self.info.rcMonitor.left) as f32 / scale,
            (self.info.rcMonitor.bottom - self.info.rcMonitor.top) as f32 / scale,
        )
    }

    fn scale_factor(&self) -> f32 {
        unsafe {
            let mut dpi_x = USER_DEFAULT_SCREEN_DPI;
            let mut dpi_y = USER_DEFAULT_SCREEN_DPI;
            let result = GetDpiForMonitor(self.hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            if result == S_OK {
                dpi_x as f32 / USER_DEFAULT_SCREEN_DPI as f32
            } else {
                1.0
            }
        }
    }

    fn is_primary(&self) -> bool {
        self.info.rcMonitor.left == 0 && self.info.rcMonitor.top == 0
    }
}
