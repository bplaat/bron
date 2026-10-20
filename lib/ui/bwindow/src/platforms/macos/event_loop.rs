/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::ptr::null;
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use super::menu::create_menu_bar;
use crate::mailbox::{Mailbox, Message};
use crate::{
    CloseRequest, EventLoopBuilder, LogicalPoint, LogicalSize, NativeEvent as Event, Theme,
    WindowEvent,
};

thread_local! {
    static EXIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

// MARK: AppDelegate
struct AppDelegateIvars {
    mailbox: Arc<Mailbox>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = AppDelegateIvars]
    struct AppDelegate;

    impl AppDelegate {
        #[unsafe(method_id(init))]
        fn _init(this: Allocated<Self>) -> Option<Retained<Self>> {
            unsafe {
                msg_send![
                    super(this.set_ivars(AppDelegateIvars {
                        mailbox: Arc::new(Mailbox::new()),
                    })),
                    init
                ]
            }
        }

        #[unsafe(method(applicationDidFinishLaunching:))]
        fn _did_finish_launching(&self, notification: *mut Object) { self.did_finish_launching(notification); }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        const fn _should_terminate(&self, _: *mut Object) -> Bool { Bool::NO }

        #[unsafe(method(applicationShouldTerminate:))]
        fn _application_should_terminate(&self, _: *mut Object) -> u64 { self.application_should_terminate() }

        #[unsafe(method(application:openURLs:))]
        fn _open_urls(&self, _: *mut Object, urls: *mut Object) { self.open_urls(urls); }

        #[unsafe(method(drainUserEvents))]
        fn _drain_user_events(&self) {
            while let Some(message) = self.ivars().mailbox.pop() {
                match message {
                    Message::User(value) => send_event(Event::UserEvent(value)),
                    Message::Exit => { stop(); break; }
                }
            }
        }

        #[unsafe(method(openAboutDialog:))]
        fn _open_about_dialog(&self, _: *mut Object) { self.open_about_dialog(); }

        #[cfg(feature = "menu")]
        #[unsafe(method(menuItemSelected:))]
        fn _menu_item_selected(&self, sender: *mut Object) { self.menu_item_selected(sender); }
    }
);

impl AppDelegate {
    fn application_should_terminate(&self) -> u64 {
        let windows: *mut Object = unsafe { msg_send![NSApp, windows] };
        let count: usize = unsafe { msg_send![windows, count] };
        let ids: Vec<_> = (0..count)
            .filter_map(|index| {
                let window: *mut Object = unsafe { msg_send![windows, objectAtIndex:index] };
                super::window::window_id(window)
            })
            .collect();
        if ids.is_empty() {
            stop();
        }
        let remaining = Rc::new(Cell::new(ids.len()));
        let prevented = Rc::new(Cell::new(false));
        for id in ids {
            let request = CloseRequest::new();
            let remaining = remaining.clone();
            let prevented = prevented.clone();
            crate::dispatch::send_then(
                Event::Window(id, WindowEvent::CloseRequested(request.clone())),
                move || {
                    prevented.set(prevented.get() || request.default_prevented());
                    remaining.set(remaining.get() - 1);
                    if remaining.get() == 0 && !prevented.get() {
                        stop();
                    }
                },
            );
        }
        0 // NSTerminateCancel: return from run instead of terminating the process.
    }

    fn did_finish_launching(&self, notification: *mut Object) {
        unsafe {
            let application: *mut Object = msg_send![notification, object];
            let _: Bool = msg_send![application, setActivationPolicy:NS_APPLICATION_ACTIVATION_POLICY_REGULAR];
            let _: () = msg_send![application, activateIgnoringOtherApps:Bool::YES];

            let windows: *mut Object = msg_send![application, windows];
            let windows_count: usize = msg_send![windows, count];
            for i in 0..windows_count {
                let window: *mut Object = msg_send![windows, objectAtIndex:i];
                if let Some(window_id) = super::window::window_id(window) {
                    let _: () = msg_send![window, makeKeyAndOrderFront:null::<Object>()];
                    send_event(Event::Window(window_id, WindowEvent::Create));
                }
            }
        }
    }

    fn open_urls(&self, urls: *mut Object) {
        let mut paths = Vec::new();
        unsafe {
            let count: usize = msg_send![urls, count];
            for index in 0..count {
                let url: *mut Object = msg_send![urls, objectAtIndex:index];
                let is_file_url: Bool = msg_send![url, isFileURL];
                if is_file_url == Bool::YES {
                    let path: NSString = msg_send![url, path];
                    paths.push(PathBuf::from(path.to_string()));
                }
            }
        }
        if !paths.is_empty() {
            send_event(Event::MacosOpenFiles(paths));
        }
    }

    fn open_about_dialog(&self) {
        let _: () = unsafe { msg_send![NSApp, orderFrontStandardAboutPanel:null::<Object>()] };
    }

    #[cfg(feature = "menu")]
    fn menu_item_selected(&self, sender: *mut Object) {
        let action: NSString = unsafe { msg_send![sender, representedObject] };
        send_event(Event::MacosMenuItem(action.to_string()));
    }
}

// MARK: EventLoop
pub(crate) struct PlatformEventLoop {
    application: Retained<Object>,
    delegate: Retained<AppDelegate>,
    theme: Theme,
}

impl PlatformEventLoop {
    pub(crate) fn new(mut builder: EventLoopBuilder) -> Self {
        // Create AppDelegate instance (registers class lazily on first call)
        let app_delegate: Retained<AppDelegate> = unsafe { msg_send![AppDelegate::class(), new] };

        // Get application
        let application = unsafe {
            let application: Retained<Object> = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![&application, setDelegate:&*app_delegate];
            application
        };

        // Create menu
        unsafe {
            create_menu_bar(
                application.as_ptr(),
                app_delegate.as_ptr().cast::<Object>(),
                &mut builder,
            );
        }

        Self {
            application,
            delegate: app_delegate,
            theme: system_theme(),
        }
    }
}

impl Drop for PlatformEventLoop {
    fn drop(&mut self) {
        crate::dispatch::shutdown(&self.delegate.ivars().mailbox);
        // Menu targets and NSApplication.delegate are non-owning. Clear the application-owned
        // references before our retained delegate is dropped.
        let current: *mut Object = unsafe { msg_send![&self.application, delegate] };
        if current == self.delegate.as_ptr().cast::<Object>() {
            unsafe {
                let _: () =
                    msg_send![&self.application, setMainMenu:std::ptr::null_mut::<Object>()];
                let _: () =
                    msg_send![&self.application, setWindowsMenu:std::ptr::null_mut::<Object>()];
                let _: () =
                    msg_send![&self.application, setHelpMenu:std::ptr::null_mut::<Object>()];
                let _: () =
                    msg_send![&self.application, setServicesMenu:std::ptr::null_mut::<Object>()];
                let _: () =
                    msg_send![&self.application, setDelegate:std::ptr::null_mut::<Object>()];
            }
        }
    }
}

// MARK: Theme
fn system_theme() -> Theme {
    unsafe {
        let application: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let appearance: *mut Object = msg_send![application, effectiveAppearance];
        let name: NSString = msg_send![appearance, name];
        if name.to_string().contains("Dark") {
            Theme::Dark
        } else {
            Theme::Light
        }
    }
}

impl crate::EventLoopInterface for PlatformEventLoop {
    fn theme(&self) -> Theme {
        self.theme
    }

    fn primary_monitor(&self) -> PlatformMonitor {
        unsafe {
            let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
            PlatformMonitor::new(screen)
        }
    }

    fn available_monitors(&self) -> Vec<PlatformMonitor> {
        let mut monitors = Vec::new();
        unsafe {
            let screens: *mut Object = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            for i in 0..count {
                let screen: *mut Object = msg_send![screens, objectAtIndex:i];
                monitors.push(PlatformMonitor::new(screen));
            }
        }
        monitors
    }

    fn run(self, event_handler: impl FnMut(Event) + 'static) -> i32 {
        crate::dispatch::start(event_handler);
        autoreleasepool(|_| unsafe {
            if !EXIT_REQUESTED.get() {
                let _: () = msg_send![&self.application, run];
            }
        });
        0
    }

    fn create_proxy(&self) -> PlatformEventLoopProxy {
        PlatformEventLoopProxy::new(self.delegate.clone())
    }
}

pub(crate) use crate::dispatch::send as send_event;

pub(super) fn allow_termination_if_last_window(closing_window: *mut Object) {
    let app_delegate: *mut Object = unsafe { msg_send![NSApp, delegate] };
    if app_delegate.is_null() {
        return;
    }
    let app_delegate_class = AppDelegate::class().cast::<AnyClass>();
    let is_ours: Bool = unsafe { msg_send![app_delegate, isKindOfClass:app_delegate_class] };
    if is_ours == Bool::NO {
        return;
    }
    let app_delegate = unsafe { &*(app_delegate as *const AppDelegate) };
    let windows: *mut Object = unsafe { msg_send![NSApp, windows] };
    let count: usize = unsafe { msg_send![windows, count] };
    let has_other_window = (0..count).any(|index| unsafe {
        let window: *mut Object = msg_send![windows, objectAtIndex:index];
        window != closing_window && super::window::window_id(window).is_some()
    });
    if !has_other_window {
        stop();
    }
}

pub(super) fn stop() {
    EXIT_REQUESTED.set(true);
    let _: () = unsafe { msg_send![NSApp, stop:null::<Object>()] };
    // Wake AppKit's event fetch so stop also works from a main-run-loop callback.
    let event: *mut Object = unsafe {
        msg_send![class!(NSEvent), otherEventWithType:15usize,
            location:NSPoint::new(0.0, 0.0), modifierFlags:0usize,
            timestamp:0.0f64, windowNumber:0isize, context:null::<Object>(),
            subtype:0i16, data1:0isize, data2:0isize]
    };
    let _: () = unsafe { msg_send![NSApp, postEvent:event, atStart:Bool::YES] };
}

// MARK: EventLoopProxy
pub(crate) struct PlatformEventLoopProxy {
    delegate: Retained<AppDelegate>,
    mailbox: Arc<Mailbox>,
}

// SAFETY: Proxies only enqueue messages and schedule a selector on the main thread.
unsafe impl Send for PlatformEventLoopProxy {}
// SAFETY: The mailbox is synchronized; no delegate ivars are accessed through proxies.
unsafe impl Sync for PlatformEventLoopProxy {}

impl PlatformEventLoopProxy {
    fn new(delegate: Retained<AppDelegate>) -> Self {
        let mailbox = delegate.ivars().mailbox.clone();
        Self { delegate, mailbox }
    }

    fn send(&self, message: Message) -> bool {
        self.mailbox.send(message, || {
            let _: () = unsafe {
                msg_send![&self.delegate, performSelectorOnMainThread:sel!(drainUserEvents),
                    withObject:null::<Object>(), waitUntilDone:Bool::NO]
            };
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

// MARK: Monitor
pub(crate) struct PlatformMonitor {
    pub(crate) screen: Retained<Object>,
}

impl PlatformMonitor {
    pub(crate) fn new(screen: *mut Object) -> Self {
        Self {
            // SAFETY: NSScreen APIs returned a live object which is retained for this handle.
            screen: unsafe { Retained::retain(screen) }.expect("NSScreen returned null"),
        }
    }
}

impl crate::MonitorInterface for PlatformMonitor {
    fn name(&self) -> String {
        let name: NSString = unsafe { msg_send![&self.screen, localizedName] };
        name.to_string()
    }

    fn position(&self) -> LogicalPoint {
        let frame: NSRect = unsafe { msg_send![&self.screen, frame] };
        LogicalPoint::new(frame.origin.x as f32, frame.origin.y as f32)
    }

    fn size(&self) -> LogicalSize {
        let frame: NSRect = unsafe { msg_send![&self.screen, frame] };
        LogicalSize::new(frame.size.width as f32, frame.size.height as f32)
    }

    fn scale_factor(&self) -> f32 {
        let backing_scale_factor: f64 = unsafe { msg_send![&self.screen, backingScaleFactor] };
        backing_scale_factor as f32
    }

    fn is_primary(&self) -> bool {
        let main_screen: *mut Object = unsafe { msg_send![class!(NSScreen), mainScreen] };
        self.screen.as_ptr() == main_screen
    }
}
