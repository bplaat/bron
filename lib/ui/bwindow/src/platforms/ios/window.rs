/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;
use std::rc::Rc;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use super::content::ContentView;
use super::event_loop::{send_event, theme_from_traits};
use super::headers::*;
use crate::content::ContentHost;
use crate::{LogicalInsets, LogicalPoint, LogicalSize, Theme, WindowBuilder, WindowEvent};

const UI_USER_INTERFACE_STYLE_LIGHT: isize = 1;
const UI_USER_INTERFACE_STYLE_DARK: isize = 2;

thread_local! {
    // The connected scene and the open windows, which get a UIWindow while a scene is connected
    static SCENE: Cell<*mut Object> = const { Cell::new(null_mut()) };
    static CONTROLLERS: RefCell<Vec<Retained<WindowViewController>>> = const { RefCell::new(Vec::new()) };
}

// MARK: WindowViewController
struct WindowViewControllerIvars {
    host: Rc<ContentHost>,
    size: Cell<CGSize>,
    window: RefCell<Option<Retained<Object>>>,
    created: Cell<bool>,
}

define_class!(
    #[unsafe(super(UIViewController))]
    #[ivars = WindowViewControllerIvars]
    struct WindowViewController;

    impl WindowViewController {
        #[unsafe(method(viewDidLayoutSubviews))]
        fn _view_did_layout_subviews(&self) { self.view_did_layout_subviews(); }

        #[unsafe(method(viewSafeAreaInsetsDidChange))]
        fn _view_safe_area_insets_did_change(&self) {
            let _: () = unsafe { msg_send![super(self), viewSafeAreaInsetsDidChange] };
            let host = &self.ivars().host;
            if !host.is_closed() {
                send_event(crate::Event::Window(
                    host.id(),
                    WindowEvent::SafeAreaChanged(self.safe_area()),
                ));
            }
        }

        #[unsafe(method(traitCollectionDidChange:))]
        fn _trait_collection_did_change(&self, previous: *mut Object) {
            let _: () = unsafe { msg_send![super(self), traitCollectionDidChange:previous] };
            self.update_theme();
        }
    }
);

impl WindowViewController {
    fn view_did_layout_subviews(&self) {
        let host = &self.ivars().host;
        if host.is_closed() {
            return;
        }
        self.update_theme();
        let view: *mut Object = unsafe { msg_send![self, view] };
        let bounds: CGRect = unsafe { msg_send![view, bounds] };
        let previous = self.ivars().size.replace(bounds.size);
        if previous.width == bounds.size.width && previous.height == bounds.size.height {
            return;
        }
        let traits: *mut Object = unsafe { msg_send![self, traitCollection] };
        let scale: f64 = unsafe { msg_send![traits, displayScale] };
        host.resize(
            (bounds.size.width * scale).round() as u32,
            (bounds.size.height * scale).round() as u32,
        );
        if !host.is_closed() {
            send_event(crate::Event::Window(
                host.id(),
                WindowEvent::Resize(LogicalSize::new(
                    bounds.size.width as f32,
                    bounds.size.height as f32,
                )),
            ));
        }
    }

    fn safe_area(&self) -> LogicalInsets {
        let view: *mut Object = unsafe { msg_send![self, view] };
        let insets: UIEdgeInsets = unsafe { msg_send![view, safeAreaInsets] };
        LogicalInsets::new(
            insets.top as f32,
            insets.right as f32,
            insets.bottom as f32,
            insets.left as f32,
        )
    }

    fn update_theme(&self) {
        let traits: *mut Object = unsafe { msg_send![self, traitCollection] };
        self.ivars().host.theme_changed(theme_from_traits(traits));
        // Also report display scale changes, like moving to an external display.
        let scale: f64 = unsafe { msg_send![traits, displayScale] };
        if scale > 0.0 {
            self.ivars().host.scale_factor_changed(scale as f32);
        }
    }
}

/// Show the open windows when UIKit connects the app's scene, also after a reconnect.
pub(super) fn connect_scene(scene: *mut Object) {
    SCENE.set(scene);
    for controller in CONTROLLERS.with_borrow(Clone::clone) {
        show_window(scene, &controller);
    }
}

/// Release the UIWindows of a disconnected scene, the controllers move to the next scene.
pub(super) fn disconnect_scene() {
    SCENE.set(null_mut());
    for controller in CONTROLLERS.with_borrow(Clone::clone) {
        controller.ivars().window.take();
    }
}

fn show_window(scene: *mut Object, controller: &WindowViewController) {
    let window = unsafe {
        let window: Allocated<Object> = msg_send![class!(UIWindow), alloc];
        let window: Retained<Object> = msg_send![window, initWithWindowScene:scene];
        let _: () = msg_send![&window, setRootViewController:controller];
        let _: () = msg_send![&window, makeKeyAndVisible];
        window
    };
    controller.ivars().window.replace(Some(window));
    if !controller.ivars().created.replace(true) {
        send_event(crate::Event::Window(
            controller.ivars().host.id(),
            WindowEvent::Create,
        ));
    }
}

fn ui_color(color: u32) -> *mut Object {
    unsafe {
        msg_send![class!(UIColor), colorWithRed:((color >> 16) & 0xFF) as f64 / 255.0,
            green:((color >> 8) & 0xFF) as f64 / 255.0,
            blue:(color & 0xFF) as f64 / 255.0, alpha:1.0]
    }
}

const fn ui_user_interface_style(theme: Theme) -> isize {
    match theme {
        Theme::Light => UI_USER_INTERFACE_STYLE_LIGHT,
        Theme::Dark => UI_USER_INTERFACE_STYLE_DARK,
    }
}

// MARK: PlatformWindow
pub(super) struct PlatformWindowData {
    window_id: crate::WindowId,
    controller: Retained<WindowViewController>,
}

pub(crate) struct PlatformWindow(Box<PlatformWindowData>);

impl PlatformWindow {
    pub(crate) const fn id(&self) -> crate::WindowId {
        self.0.window_id
    }

    /// Windows always fill the screen, so size, position and resizing options are ignored.
    pub(crate) fn new(builder: &WindowBuilder, host: Rc<ContentHost>) -> Self {
        let controller: Allocated<WindowViewController> =
            unsafe { msg_send![WindowViewController::class(), alloc] };
        let controller: Retained<WindowViewController> = unsafe {
            msg_send![
                super(controller.set_ivars(WindowViewControllerIvars {
                    host: host.clone(),
                    size: Cell::new(CGSize::new(0.0, 0.0)),
                    window: RefCell::new(None),
                    created: Cell::new(false),
                })),
                init
            ]
        };
        // The shared content view renderers draw into, it receives the interaction.
        let content = ContentView::new(host.event_sender());
        let _: () = unsafe { msg_send![&controller, setView:&*content] };
        let view: *mut Object = content.as_ptr().cast();
        unsafe {
            let _: () = msg_send![&controller, setTitle:&*NSString::new(&builder.title)];
            if let Some(color) = builder.background_color {
                let _: () = msg_send![view, setBackgroundColor:ui_color(color)];
            }
            if let Some(theme) = builder.theme {
                let _: () = msg_send![&controller, setOverrideUserInterfaceStyle:ui_user_interface_style(theme)];
            }
        }
        host.set_handle(crate::NativeWindowHandle::UIKit(view.cast()));
        host.set_content(crate::NativeContentHandle::UIKit(view.cast()));
        // Until the first layout reports the scale of its screen, assume the main screen.
        let screen: *mut Object = unsafe { msg_send![class!(UIScreen), mainScreen] };
        let scale: f64 = unsafe { msg_send![screen, scale] };
        host.scale_factor_changed(scale as f32);

        CONTROLLERS.with_borrow_mut(|controllers| controllers.push(controller.clone()));
        let scene = SCENE.get();
        if !scene.is_null() {
            show_window(scene, &controller);
        }
        PlatformWindow(Box::new(PlatformWindowData {
            window_id: builder.window_id,
            controller,
        }))
    }
}

impl crate::WindowInterface for PlatformWindow {
    fn close(&mut self) {
        if let Some(window) = self.0.controller.ivars().window.take() {
            let _: () = unsafe { msg_send![&window, setHidden:Bool::YES] };
        }
        CONTROLLERS.with_borrow_mut(|controllers| {
            controllers.retain(|controller| controller.as_ptr() != self.0.controller.as_ptr());
        });
    }

    fn set_title(&mut self, title: impl AsRef<str>) {
        let _: () = unsafe { msg_send![&self.0.controller, setTitle:&*NSString::new(title)] };
    }

    fn position(&self) -> LogicalPoint {
        LogicalPoint::new(0.0, 0.0)
    }

    fn size(&self) -> LogicalSize {
        let view: *mut Object = unsafe { msg_send![&self.0.controller, view] };
        let bounds: CGRect = unsafe { msg_send![view, bounds] };
        LogicalSize::new(bounds.size.width as f32, bounds.size.height as f32)
    }

    fn safe_area(&self) -> LogicalInsets {
        self.0.controller.safe_area()
    }

    fn set_position(&mut self, _point: LogicalPoint) {}

    fn set_size(&mut self, _size: LogicalSize) {}

    fn set_min_size(&mut self, _min_size: LogicalSize) {}

    fn set_resizable(&mut self, _resizable: bool) {}

    fn set_theme(&mut self, theme: Theme) {
        let _: () = unsafe {
            msg_send![&self.0.controller, setOverrideUserInterfaceStyle:ui_user_interface_style(theme)]
        };
    }

    fn set_background_color(&mut self, color: u32) {
        let view: *mut Object = unsafe { msg_send![&self.0.controller, view] };
        let _: () = unsafe { msg_send![view, setBackgroundColor:ui_color(color)] };
    }
}
