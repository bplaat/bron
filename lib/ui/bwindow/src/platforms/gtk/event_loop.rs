/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fs::File;
use std::mem::MaybeUninit;
use std::process::exit;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex};
use std::{env, fs, iter};

use super::headers::*;
use crate::mailbox::{Mailbox, Message};
use crate::{AppId, EventLoopBuilder, LogicalPoint, LogicalSize, NativeEvent as Event, Theme};

// MARK: EventLoop
pub(crate) struct PlatformEventLoop {
    theme: Theme,
    wake: Arc<Wake>,
}

pub(super) static mut APP_ID: Option<AppId> = None;

impl PlatformEventLoop {
    pub(crate) fn new(builder: EventLoopBuilder) -> Self {
        let app_id_name = builder.app_id.as_ref().map(|app_id| {
            CString::new(format!(
                "{}.{}.{}",
                app_id.qualifier, app_id.organization, app_id.application
            ))
            .expect("Can't convert app id to CString")
        });

        // Ensure single instance
        // FIXME: Use GtkApplication for this
        if let Some(app_id) = builder.app_id {
            if builder.single_instance {
                let lock_file = env::temp_dir()
                    .join(format!(
                        "{}.{}.{}",
                        app_id.qualifier, app_id.organization, app_id.application
                    ))
                    .join(".lock");
                if let Some(parent) = lock_file.parent() {
                    fs::create_dir_all(parent).expect("Failed to create lock file directory");
                }
                let file = File::create(&lock_file).expect("Failed to open lock file");
                if file.try_lock().is_err() {
                    exit(0);
                }
                std::mem::forget(file);
            }
            unsafe { APP_ID = Some(app_id) };
        }

        // Init GTK
        unsafe {
            if let Some(ref app_id) = app_id_name {
                g_set_prgname(app_id.as_ptr());
            }

            let args = env::args()
                .map(|arg| CString::new(arg.as_str()).expect("Can't convert to CString"))
                .collect::<Vec<CString>>();
            let mut argc = args.len() as i32;
            let mut argv: Vec<*mut c_char> = args
                .iter()
                .map(|arg| arg.as_ptr() as *mut c_char)
                .chain(iter::once(null_mut()))
                .collect();
            let mut argv_ptr = argv.as_mut_ptr();
            gtk_init(&mut argc, &mut argv_ptr);

            if let Some(ref app_id) = app_id_name {
                gtk_window_set_default_icon_name(app_id.as_ptr());
            }
        }
        Self {
            theme: system_theme(),
            wake: Arc::new(Wake {
                mailbox: Mailbox::new(),
                source: Mutex::new(None),
            }),
        }
    }
}

// MARK: Theme
pub(super) fn system_theme() -> Theme {
    let settings = unsafe { gtk_settings_get_default() };
    let mut prefer_dark = 0i32;
    let mut theme_name: *mut c_char = null_mut();
    unsafe {
        g_object_get(
            settings as *mut GObject,
            c"gtk-application-prefer-dark-theme".as_ptr(),
            &mut prefer_dark,
            c"gtk-theme-name".as_ptr(),
            &mut theme_name,
            null::<c_void>(),
        );
    }
    let theme_name_is_dark = if theme_name.is_null() {
        false
    } else {
        let is_dark = unsafe { CStr::from_ptr(theme_name) }
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("dark");
        unsafe { g_free(theme_name as *mut c_void) };
        is_dark
    };
    if prefer_dark != 0 || theme_name_is_dark {
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
        cfg_select! {
            gtk3_22 => unsafe {
                let mut m = gdk_display_get_primary_monitor(gdk_display_get_default());
                if m.is_null() {
                    m = gdk_display_get_monitor(gdk_display_get_default(), 0);
                }
                PlatformMonitor::new(m)
            },
            _ => unsafe {
                let screen = gdk_screen_get_default();
                let idx = gdk_screen_get_primary_monitor(screen);
                PlatformMonitor::new(idx)
            },
        }
    }

    fn available_monitors(&self) -> Vec<PlatformMonitor> {
        cfg_select! {
            gtk3_22 => unsafe {
                let display = gdk_display_get_default();
                (0..gdk_display_get_n_monitors(display))
                    .map(|i| PlatformMonitor::new(gdk_display_get_monitor(display, i)))
                    .collect()
            },
            _ => unsafe {
                let screen = gdk_screen_get_default();
                (0..gdk_screen_get_n_monitors(screen))
                    .map(PlatformMonitor::new)
                    .collect()
            },
        }
    }

    fn run(self, event_handler: impl FnMut(Event) + 'static) -> i32 {
        crate::dispatch::start(event_handler);

        // Start event loop
        if !EXIT_REQUESTED.get() {
            unsafe { gtk_main() };
        }
        0
    }

    fn create_proxy(&self) -> PlatformEventLoopProxy {
        PlatformEventLoopProxy(self.wake.clone())
    }
}

pub(crate) use crate::dispatch::send as send_event;

thread_local! {
    static EXIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn stop() {
    EXIT_REQUESTED.set(true);
    // Startup callbacks can close the last window before gtk_main starts.
    unsafe {
        if gtk_main_level() != 0 {
            gtk_main_quit();
        }
    }
}

struct Wake {
    mailbox: Mailbox,
    source: Mutex<Option<u32>>,
}

impl Drop for PlatformEventLoop {
    fn drop(&mut self) {
        crate::dispatch::shutdown(&self.wake.mailbox);
        if let Some(id) = self
            .wake
            .source
            .lock()
            .expect("event source poisoned")
            .take()
        {
            unsafe { g_source_remove(id) };
        }
    }
}

// MARK: EventLoopProxy
pub(crate) struct PlatformEventLoopProxy(Arc<Wake>);

impl PlatformEventLoopProxy {
    fn send(&self, message: Message) -> bool {
        self.0.mailbox.send(message, || {
            let mut source = self.0.source.lock().expect("event source poisoned");
            if source.is_none() {
                let data = Arc::into_raw(self.0.clone()) as *mut c_void;
                let id = unsafe { g_idle_add_full(0, wake_callback, data, drop_wake) };
                if id == 0 {
                    unsafe { drop_wake(data) };
                    return false;
                }
                *source = Some(id);
            }
            true
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

extern "C" fn wake_callback(data: *mut c_void) -> i32 {
    let wake = unsafe { &*data.cast::<Wake>() };
    *wake.source.lock().expect("event source poisoned") = None;
    while let Some(message) = wake.mailbox.pop() {
        match message {
            Message::User(value) => send_event(Event::UserEvent(value)),
            Message::Exit => {
                stop();
                break;
            }
        }
    }
    0
}

extern "C" fn drop_wake(data: *mut c_void) {
    unsafe { drop(Arc::from_raw(data.cast::<Wake>())) };
}

// MARK: Monitor
cfg_select! {
    gtk3_22 => {
        pub(crate) struct PlatformMonitor {
            pub(crate) monitor: *mut GdkMonitor,
        }

        impl PlatformMonitor {
            pub(crate) const fn new(monitor: *mut GdkMonitor) -> Self {
                Self { monitor }
            }
        }
    }
    _ => {
        pub(crate) struct PlatformMonitor {
            pub(crate) index: i32,
        }

        impl PlatformMonitor {
            pub(crate) const fn new(index: i32) -> Self {
                Self { index }
            }
        }
    }
}

impl PlatformMonitor {
    // Returns the screen rectangle for this monitor.
    pub(super) fn rect(&self) -> GdkRectangle {
        cfg_select! {
            gtk3_22 => {
                let mut rect = MaybeUninit::<GdkRectangle>::uninit();
                unsafe { gdk_monitor_get_geometry(self.monitor, rect.as_mut_ptr()) };
                unsafe { rect.assume_init() }
            }
            _ => {
                let screen = unsafe { gdk_screen_get_default() };
                let mut rect = MaybeUninit::<GdkRectangle>::uninit();
                unsafe { gdk_screen_get_monitor_geometry(screen, self.index, rect.as_mut_ptr()) };
                unsafe { rect.assume_init() }
            }
        }
    }
}

impl crate::MonitorInterface for PlatformMonitor {
    fn name(&self) -> String {
        cfg_select! {
            gtk3_22 => unsafe {
                CStr::from_ptr(gdk_monitor_get_model(self.monitor))
                    .to_string_lossy()
                    .into_owned()
            },
            _ => unsafe {
                let ptr = gdk_screen_get_monitor_plug_name(gdk_screen_get_default(), self.index);
                if ptr.is_null() {
                    format!("Monitor {}", self.index)
                } else {
                    let name = CStr::from_ptr(ptr).to_string_lossy().into_owned();
                    g_free(ptr as *mut c_void);
                    name
                }
            },
        }
    }

    fn position(&self) -> LogicalPoint {
        let rect = self.rect();
        let primary_rect = primary_monitor_rect();
        LogicalPoint::new(
            (rect.x - primary_rect.x) as f32,
            (rect.y - primary_rect.y) as f32,
        )
    }

    fn size(&self) -> LogicalSize {
        let rect = self.rect();
        LogicalSize::new(rect.width as f32, rect.height as f32)
    }

    fn scale_factor(&self) -> f32 {
        cfg_select! {
            gtk3_22 => unsafe { gdk_monitor_get_scale_factor(self.monitor) as f32 },
            _ => unsafe {
                gdk_screen_get_monitor_scale_factor(gdk_screen_get_default(), self.index) as f32
            },
        }
    }

    fn is_primary(&self) -> bool {
        cfg_select! {
            gtk3_22 => unsafe { gdk_monitor_is_primary(self.monitor) },
            _ => unsafe { gdk_screen_get_primary_monitor(gdk_screen_get_default()) == self.index },
        }
    }
}

// Returns the screen rectangle of the primary monitor.
pub(super) fn primary_monitor_rect() -> GdkRectangle {
    cfg_select! {
        gtk3_22 => unsafe {
            let display = gdk_display_get_default();
            let mut m = gdk_display_get_primary_monitor(display);
            if m.is_null() {
                m = gdk_display_get_monitor(display, 0);
            }
            let mut r = MaybeUninit::<GdkRectangle>::uninit();
            gdk_monitor_get_geometry(m, r.as_mut_ptr());
            r.assume_init()
        },
        _ => unsafe {
            let screen = gdk_screen_get_default();
            let idx = gdk_screen_get_primary_monitor(screen);
            // Normalize: -1 means no primary set, fall back to monitor 0.
            let idx = if idx < 0 { 0 } else { idx };
            let mut r = MaybeUninit::<GdkRectangle>::uninit();
            gdk_screen_get_monitor_geometry(screen, idx, r.as_mut_ptr());
            r.assume_init()
        },
    }
}
