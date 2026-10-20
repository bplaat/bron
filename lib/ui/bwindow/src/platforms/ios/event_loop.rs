/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{c_char, c_int};
use std::ptr::{null, null_mut};
use std::sync::Arc;

use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use crate::mailbox::{Mailbox, Message};
use crate::{EventLoopBuilder, LogicalPoint, LogicalSize, NativeEvent as Event, Theme};

#[link(name = "UIKit", kind = "framework")]
unsafe extern "C" {
    fn UIApplicationMain(
        argc: c_int,
        argv: *mut *mut c_char,
        principal_class_name: *mut Object,
        delegate_class_name: &Object,
    ) -> c_int;
}

const UI_USER_INTERFACE_STYLE_DARK: isize = 2;

// MARK: AppDelegate
define_class!(
    #[unsafe(super(UIResponder))]
    struct AppDelegate;

    impl AppDelegate {
        #[unsafe(method(application:configurationForConnectingSceneSession:options:))]
        fn _configuration_for_connecting_scene_session(&self, _: *mut Object, session: *mut Object, _: *mut Object) -> *mut Object {
            let role: *mut Object = unsafe { msg_send![session, role] };
            let configuration: *mut Object = unsafe {
                msg_send![class!(UISceneConfiguration), configurationWithName:null_mut::<Object>(), sessionRole:role]
            };
            let _: () = unsafe { msg_send![configuration, setDelegateClass:SceneDelegate::class().cast::<AnyClass>()] };
            configuration
        }
    }
);

// MARK: SceneDelegate
define_class!(
    #[unsafe(super(UIResponder))]
    struct SceneDelegate;

    impl SceneDelegate {
        #[unsafe(method(scene:willConnectToSession:options:))]
        fn _will_connect_to_session(&self, scene: *mut Object, _: *mut Object, _: *mut Object) {
            super::window::connect_scene(scene);
        }

        #[unsafe(method(sceneDidDisconnect:))]
        fn _scene_did_disconnect(&self, _: *mut Object) {
            super::window::disconnect_scene();
        }
    }
);

// MARK: EventLoopWaker
struct EventLoopWakerIvars {
    mailbox: Arc<Mailbox>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = EventLoopWakerIvars]
    struct EventLoopWaker;

    impl EventLoopWaker {
        #[unsafe(method(drainUserEvents))]
        fn _drain_user_events(&self) {
            while let Some(message) = self.ivars().mailbox.pop() {
                match message {
                    Message::User(value) => send_event(Event::UserEvent(value)),
                    Message::Exit => stop(&self.ivars().mailbox),
                }
            }
        }
    }
);

// MARK: EventLoop
pub(crate) struct PlatformEventLoop {
    waker: Retained<EventLoopWaker>,
}

impl PlatformEventLoop {
    pub(crate) fn new(_builder: EventLoopBuilder) -> Self {
        let waker: Allocated<EventLoopWaker> = unsafe { msg_send![EventLoopWaker::class(), alloc] };
        let waker: Retained<EventLoopWaker> = unsafe {
            msg_send![
                super(waker.set_ivars(EventLoopWakerIvars {
                    mailbox: Arc::new(Mailbox::new()),
                })),
                init
            ]
        };
        Self { waker }
    }
}

impl Drop for PlatformEventLoop {
    fn drop(&mut self) {
        crate::dispatch::shutdown(&self.waker.ivars().mailbox);
    }
}

impl crate::EventLoopInterface for PlatformEventLoop {
    fn theme(&self) -> Theme {
        let traits: *mut Object =
            unsafe { msg_send![class!(UITraitCollection), currentTraitCollection] };
        theme_from_traits(traits)
    }

    fn primary_monitor(&self) -> PlatformMonitor {
        PlatformMonitor::new(unsafe { msg_send![class!(UIScreen), mainScreen] })
    }

    fn available_monitors(&self) -> Vec<PlatformMonitor> {
        vec![self.primary_monitor()]
    }

    fn run(self, event_handler: impl FnMut(Event) + 'static) -> i32 {
        crate::dispatch::start(event_handler);
        // Register the delegate classes before UIKit looks them up by name.
        AppDelegate::class();
        unsafe {
            let protocol = objc2::ffi::objc_getProtocol(c"UIWindowSceneDelegate".as_ptr());
            objc2::ffi::class_addProtocol(SceneDelegate::class().cast::<AnyClass>(), protocol);
        }
        let delegate_class_name = NSString::new("AppDelegate");
        autoreleasepool(|_| unsafe {
            UIApplicationMain(0, null_mut(), null_mut(), &delegate_class_name)
        })
    }

    fn create_proxy(&self) -> PlatformEventLoopProxy {
        PlatformEventLoopProxy {
            waker: self.waker.clone(),
            mailbox: self.waker.ivars().mailbox.clone(),
        }
    }
}

pub(crate) use crate::dispatch::send as send_event;

pub(super) fn theme_from_traits(traits: *mut Object) -> Theme {
    let style: isize = unsafe { msg_send![traits, userInterfaceStyle] };
    if style == UI_USER_INTERFACE_STYLE_DARK {
        Theme::Dark
    } else {
        Theme::Light
    }
}

// iOS apps have no event loop exit, so deliver LoopExiting and end the process.
fn stop(mailbox: &Mailbox) -> ! {
    crate::dispatch::shutdown(mailbox);
    std::process::exit(0);
}

// MARK: EventLoopProxy
pub(crate) struct PlatformEventLoopProxy {
    waker: Retained<EventLoopWaker>,
    mailbox: Arc<Mailbox>,
}

// SAFETY: Proxies only enqueue messages and schedule a selector on the main thread.
unsafe impl Send for PlatformEventLoopProxy {}
// SAFETY: The mailbox is synchronized; no waker ivars are accessed through proxies.
unsafe impl Sync for PlatformEventLoopProxy {}

impl PlatformEventLoopProxy {
    fn send(&self, message: Message) -> bool {
        self.mailbox.send(message, || {
            let _: () = unsafe {
                msg_send![&self.waker, performSelectorOnMainThread:sel!(drainUserEvents),
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
    screen: Retained<Object>,
}

impl PlatformMonitor {
    fn new(screen: *mut Object) -> Self {
        Self {
            // SAFETY: UIScreen returned a live object which is retained for this handle.
            screen: unsafe { Retained::retain(screen) }.expect("UIScreen returned null"),
        }
    }
}

impl crate::MonitorInterface for PlatformMonitor {
    fn name(&self) -> String {
        let device: *mut Object = unsafe { msg_send![class!(UIDevice), currentDevice] };
        let name: NSString = unsafe { msg_send![device, model] };
        name.to_string()
    }

    fn position(&self) -> LogicalPoint {
        LogicalPoint::new(0.0, 0.0)
    }

    fn size(&self) -> LogicalSize {
        let bounds: CGRect = unsafe { msg_send![&self.screen, bounds] };
        LogicalSize::new(bounds.size.width as f32, bounds.size.height as f32)
    }

    fn scale_factor(&self) -> f32 {
        let scale: f64 = unsafe { msg_send![&self.screen, scale] };
        scale as f32
    }

    fn is_primary(&self) -> bool {
        true
    }
}
