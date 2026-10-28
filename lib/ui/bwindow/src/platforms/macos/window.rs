/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ptr::null_mut;

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use super::content::ContentView;
use super::event_loop::{allow_termination_if_last_window, has_launched, send_event};
#[cfg(feature = "file_drop")]
use super::file_drop::{perform_file_drop, register_dragged_types};
use super::headers::*;
use crate::{
    CloseRequest, CursorIcon, LogicalPoint, LogicalSize, MacosTitlebarStyle, PointerLockError,
    Theme, WindowBuilder, WindowEvent,
};

define_class!(
    #[unsafe(super(NSView))]
    pub(super) struct DraggableView;

    impl DraggableView {
        #[unsafe(method(acceptsFirstMouse:))]
        const fn _accepts_first_mouse(&self, _: *mut Object) -> Bool { Bool::YES }

        #[unsafe(method(mouseDown:))]
        fn _mouse_down(&self, event: *mut Object) {
            let this = self as *const DraggableView as *mut Object;
            let window: *mut Object = unsafe { msg_send![this, window] };
            if window.is_null() || event.is_null() {
                return;
            }

            let click_count: isize = unsafe { msg_send![event, clickCount] };
            if click_count == 2 {
                perform_titlebar_double_click(window);
            } else {
                let _: () = unsafe { msg_send![window, performWindowDragWithEvent:event] };
            }
        }
    }
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TitlebarDoubleClickAction {
    Zoom,
    Minimize,
    None,
}

impl TitlebarDoubleClickAction {
    fn from_preference(value: Option<&str>) -> Self {
        match value {
            Some("Minimize") => Self::Minimize,
            Some("None") => Self::None,
            // AppKit has no public API for the newer Fill action. Zoom is the closest system
            // action and is also the default for missing or unknown preference values.
            Some("Fill" | "Maximize") | None | Some(_) => Self::Zoom,
        }
    }
}

fn titlebar_double_click_action() -> TitlebarDoubleClickAction {
    let defaults: *mut Object = unsafe { msg_send![class!(NSUserDefaults), standardUserDefaults] };
    let global_domain: *mut Object =
        unsafe { msg_send![defaults, persistentDomainForName:ns_string!("NSGlobalDomain")] };
    if global_domain.is_null() {
        return TitlebarDoubleClickAction::Zoom;
    }

    let action: *mut Object =
        unsafe { msg_send![global_domain, objectForKey:ns_string!("AppleActionOnDoubleClick")] };
    if action.is_null() {
        return TitlebarDoubleClickAction::Zoom;
    }

    let action: NSString = unsafe { msg_send![action, copy] };
    TitlebarDoubleClickAction::from_preference(Some(&action.to_string()))
}

fn perform_titlebar_double_click(window: *mut Object) {
    let style_mask: u64 = unsafe { msg_send![window, styleMask] };
    match titlebar_double_click_action() {
        TitlebarDoubleClickAction::Zoom if style_mask & NS_WINDOW_STYLE_MASK_RESIZABLE != 0 => {
            let _: () = unsafe { msg_send![window, zoom:null_mut::<Object>()] };
        }
        TitlebarDoubleClickAction::Minimize
            if style_mask & NS_WINDOW_STYLE_MASK_MINIATURIZABLE != 0 =>
        {
            let _: () = unsafe { msg_send![window, miniaturize:null_mut::<Object>()] };
        }
        TitlebarDoubleClickAction::Zoom
        | TitlebarDoubleClickAction::Minimize
        | TitlebarDoubleClickAction::None => {}
    }
}

// MARK: WindowDelegate
struct WindowDelegateIvars {
    host: std::rc::Rc<crate::content::ContentHost>,
    traffic_light_position: Option<NSPoint>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = WindowDelegateIvars]
    struct WindowDelegate;

    impl WindowDelegate {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn appearance_changed(&self, _: *mut Object, window: *mut Object, _: *mut Object, _: *mut std::ffi::c_void) {
            let appearance: *mut Object = unsafe { msg_send![window, effectiveAppearance] };
            let name: NSString = unsafe { msg_send![appearance, name] };
            self.ivars().host.theme_changed(if name.to_string().contains("Dark") { Theme::Dark } else { Theme::Light });
        }
        #[unsafe(method(windowDidBecomeKey:))]
        fn _window_did_become_key(&self, notification: *mut Object) {
            self.layout_window_buttons(notification);
            send_event(crate::Event::Window(self.ivars().host.id(), WindowEvent::Focus));
        }

        #[unsafe(method(windowDidResignKey:))]
        fn _window_did_resign_key(&self, notification: *mut Object) {
            self.layout_window_buttons(notification);
            send_event(crate::Event::Window(self.ivars().host.id(), WindowEvent::Blur));
        }

        #[unsafe(method(windowShouldClose:))]
        fn _window_should_close(&self, window: *mut Object) -> Bool { self.window_should_close(window) }

        #[unsafe(method(windowWillClose:))]
        fn _window_will_close(&self, _: *mut Object) { self.ivars().host.close(); }

        #[unsafe(method(windowDidMove:))]
        fn _window_did_move(&self, notification: *mut Object) { self.window_did_move(notification); }

        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn _window_did_change_backing_properties(&self, notification: *mut Object) {
            let host = self.ivars().host.clone();
            self.window_did_resize(notification);
            if !host.is_closed() {
                let window: *mut Object = unsafe { msg_send![notification, object] };
                let scale: f64 = unsafe { msg_send![window, backingScaleFactor] };
                host.scale_factor_changed(scale as f32);
                send_event(crate::Event::Window(host.id(), WindowEvent::ScaleFactorChanged(scale as f32)));
                host.request_redraw();
            }
        }

        #[unsafe(method(windowDidResize:))]
        fn _window_did_resize(&self, notification: *mut Object) { self.window_did_resize(notification); }

        #[unsafe(method(windowWillEnterFullScreen:))]
        fn _window_will_enter_fullscreen(&self, notification: *mut Object) { self.window_will_enter_fullscreen(notification); }

        #[unsafe(method(windowWillExitFullScreen:))]
        fn _window_will_exit_fullscreen(&self, _: *mut Object) { self.window_will_exit_fullscreen(); }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn _window_did_exit_fullscreen(&self, notification: *mut Object) { self.window_did_exit_fullscreen(notification); }

        #[unsafe(method(windowDidFailToEnterFullScreen:))]
        fn _window_did_fail_to_enter_fullscreen(&self, notification: *mut Object) { self.window_did_fail_to_enter_fullscreen(notification); }

        #[unsafe(method(windowDidFailToExitFullScreen:))]
        fn _window_did_fail_to_exit_fullscreen(&self, notification: *mut Object) { self.window_did_fail_to_exit_fullscreen(notification); }

        #[cfg(feature = "file_drop")]
        #[unsafe(method(draggingEntered:))]
        const fn _dragging_entered(&self, _: *mut Object) -> u64 { NS_DRAG_OPERATION_COPY }

        #[cfg(feature = "file_drop")]
        #[unsafe(method(draggingUpdated:))]
        const fn _dragging_updated(&self, _: *mut Object) -> u64 { NS_DRAG_OPERATION_COPY }

        #[cfg(feature = "file_drop")]
        #[unsafe(method(prepareForDragOperation:))]
        const fn _prepare_for_drag_operation(&self, _: *mut Object) -> Bool { Bool::YES }

        #[cfg(feature = "file_drop")]
        #[unsafe(method(performDragOperation:))]
        fn _perform_drag_operation(&self, sender: *mut Object) -> Bool { unsafe { perform_file_drop(sender) } }
    }
);

impl WindowDelegate {
    /// AppKit lays out the window buttons again on focus, title and size changes
    fn layout_window_buttons(&self, notification: *mut Object) {
        if let Some(position) = self.ivars().traffic_light_position {
            let window: *mut Object = unsafe { msg_send![notification, object] };
            layout_window_buttons(window, position);
        }
    }

    fn window_should_close(&self, window: *mut Object) -> Bool {
        let Some(window) = (unsafe { Retained::retain(window) }) else {
            return Bool::NO;
        };
        let request = CloseRequest::new();
        let host = self.ivars().host.clone();
        crate::dispatch::send_then(
            crate::Event::Window(
                self.ivars().host.id(),
                WindowEvent::CloseRequested(request.clone()),
            ),
            move || {
                if !request.default_prevented() && !host.is_closed() {
                    allow_termination_if_last_window(window.as_ptr());
                    let _: () = unsafe { msg_send![&window, close] };
                }
            },
        );
        Bool::NO
    }

    fn window_did_move(&self, notification: *mut Object) {
        let window: *mut Object = unsafe { msg_send![notification, object] };
        let frame: NSRect = unsafe { msg_send![window, frame] };
        send_event(crate::Event::Window(
            self.ivars().host.id(),
            WindowEvent::Move(LogicalPoint::new(
                frame.origin.x as f32,
                frame.origin.y as f32,
            )),
        ));
    }

    fn window_did_resize(&self, notification: *mut Object) {
        let window: *mut Object = unsafe { msg_send![notification, object] };
        let content_view: *mut Object = unsafe { msg_send![window, contentView] };
        let frame: NSRect = unsafe { msg_send![content_view, frame] };
        let scale: f64 = unsafe { msg_send![window, backingScaleFactor] };
        self.layout_window_buttons(notification);
        let host = self.ivars().host.clone();
        host.resize(
            (frame.size.width * scale).round() as u32,
            (frame.size.height * scale).round() as u32,
        );
        if host.is_closed() {
            return;
        }
        send_event(crate::Event::Window(
            host.id(),
            WindowEvent::Resize(LogicalSize::new(
                frame.size.width as f32,
                frame.size.height as f32,
            )),
        ));
    }

    fn window_will_enter_fullscreen(&self, notification: *mut Object) {
        let window: *mut Object = unsafe { msg_send![notification, object] };
        set_drag_view_hidden(window, true);
        send_event(crate::Event::Window(
            self.ivars().host.id(),
            WindowEvent::MacosFullscreenChange(true),
        ));
    }

    fn window_will_exit_fullscreen(&self) {
        send_event(crate::Event::Window(
            self.ivars().host.id(),
            WindowEvent::MacosFullscreenChange(false),
        ));
    }

    fn window_did_exit_fullscreen(&self, notification: *mut Object) {
        let window: *mut Object = unsafe { msg_send![notification, object] };
        set_drag_view_hidden(window, false);
        self.layout_window_buttons(notification);
    }

    fn window_did_fail_to_enter_fullscreen(&self, notification: *mut Object) {
        let window: *mut Object = unsafe { msg_send![notification, object] };
        set_drag_view_hidden(window, false);
        send_event(crate::Event::Window(
            self.ivars().host.id(),
            WindowEvent::MacosFullscreenChange(false),
        ));
    }

    fn window_did_fail_to_exit_fullscreen(&self, notification: *mut Object) {
        let window: *mut Object = unsafe { msg_send![notification, object] };
        set_drag_view_hidden(window, true);
        send_event(crate::Event::Window(
            self.ivars().host.id(),
            WindowEvent::MacosFullscreenChange(true),
        ));
    }
}

fn set_drag_view_hidden(window: *mut Object, hidden: bool) {
    let has_transparent_titlebar: Bool = unsafe { msg_send![window, titlebarAppearsTransparent] };
    if has_transparent_titlebar == Bool::NO {
        return;
    }
    let content_view: *mut Object = unsafe { msg_send![window, contentView] };
    let subviews: *mut Object = unsafe { msg_send![content_view, subviews] };
    let drag_view: *mut Object = unsafe { msg_send![subviews, lastObject] };
    if drag_view.is_null() {
        return;
    }
    let class = DraggableView::class().cast::<objc2::runtime::AnyClass>();
    let is_drag_view: Bool = unsafe { msg_send![drag_view, isKindOfClass:class] };
    if is_drag_view == Bool::NO {
        return;
    }
    let hidden = if hidden { Bool::YES } else { Bool::NO };
    let _: () = unsafe { msg_send![drag_view, setHidden:hidden] };
}

// Add a titlebar drag view at the top of the flipped content view, above all content.
fn add_drag_view(window: *mut Object, content_view: *mut Object) {
    let drag_view: Retained<Object> = unsafe { msg_send![DraggableView::class(), new] };
    let bounds: NSRect = unsafe { msg_send![content_view, bounds] };
    let content_layout_rect: NSRect = unsafe { msg_send![window, contentLayoutRect] };
    let titlebar_height = bounds.size.height - content_layout_rect.size.height;
    let _: () = unsafe {
        msg_send![&drag_view, setFrame:NSRect::new(
            bounds.origin,
            NSSize::new(bounds.size.width, titlebar_height),
        )]
    };
    let _: () = unsafe {
        msg_send![&drag_view, setAutoresizingMask:NS_VIEW_WIDTH_SIZABLE | NS_VIEW_MAX_Y_MARGIN]
    };
    let _: () = unsafe { msg_send![content_view, addSubview:drag_view.as_ptr()] };
}

fn window_buttons(window: *mut Object) -> Option<[*mut Object; 3]> {
    let buttons: [*mut Object; 3] = [
        unsafe { msg_send![window, standardWindowButton:NS_WINDOW_CLOSE_BUTTON] },
        unsafe { msg_send![window, standardWindowButton:NS_WINDOW_MINIATURIZE_BUTTON] },
        unsafe { msg_send![window, standardWindowButton:NS_WINDOW_ZOOM_BUTTON] },
    ];
    (!buttons.iter().any(|button| button.is_null())).then_some(buttons)
}

/// Titlebar height that keeps the window buttons at the same distance from the top and bottom
fn traffic_light_titlebar_height(window: *mut Object, position: NSPoint) -> Option<f64> {
    let buttons = window_buttons(window)?;
    let close_frame: NSRect = unsafe { msg_send![buttons[0], frame] };
    Some(position.y * 2.0 + close_frame.size.height)
}

/// Move the window buttons to a position from the top left corner of the window
fn layout_window_buttons(window: *mut Object, position: NSPoint) {
    let style_mask: u64 = unsafe { msg_send![window, styleMask] };
    if style_mask & NS_WINDOW_STYLE_MASK_FULLSCREEN != 0 {
        return;
    }
    let (Some(buttons), Some(titlebar_height)) = (
        window_buttons(window),
        traffic_light_titlebar_height(window, position),
    ) else {
        return;
    };
    let titlebar_view: *mut Object = unsafe { msg_send![buttons[0], superview] };
    let container_view: *mut Object = unsafe { msg_send![titlebar_view, superview] };
    if container_view.is_null() {
        return;
    }

    // Resize the titlebar container to fit the buttons, anchored at the top of the window
    let window_frame: NSRect = unsafe { msg_send![window, frame] };
    let mut container_frame: NSRect = unsafe { msg_send![container_view, frame] };
    container_frame.size.height = titlebar_height;
    container_frame.origin.y = window_frame.size.height - titlebar_height;
    let _: () = unsafe { msg_send![container_view, setFrame:container_frame] };
    let _: () = unsafe {
        msg_send![titlebar_view, setFrame:NSRect::new(NSPoint::new(0.0, 0.0), container_frame.size)]
    };

    let close_frame: NSRect = unsafe { msg_send![buttons[0], frame] };
    let miniaturize_frame: NSRect = unsafe { msg_send![buttons[1], frame] };
    let spacing = miniaturize_frame.origin.x - close_frame.origin.x;
    for (index, button) in buttons.into_iter().enumerate() {
        let _: () = unsafe {
            msg_send![button, setFrameOrigin:NSPoint::new(position.x + spacing * index as f64, position.y)]
        };
    }
}

pub(super) struct PlatformWindowData {
    pub(super) window_id: crate::WindowId,
    pub(super) window: Retained<Object>,
    content_view: Retained<ContentView>,
    _delegate: Retained<Object>,
    pub(super) background_color: Option<u32>,
    traffic_light_position: Option<NSPoint>,
}

pub(crate) struct PlatformWindow(pub(super) Box<PlatformWindowData>);

impl PlatformWindow {
    pub(crate) const fn id(&self) -> crate::WindowId {
        self.0.window_id
    }

    pub(crate) fn new(
        builder: &WindowBuilder,
        host: std::rc::Rc<crate::content::ContentHost>,
    ) -> Self {
        // Register WindowDelegate class and configure NSWindow (idempotent)
        let _: () =
            unsafe { msg_send![class!(NSWindow), setAllowsAutomaticWindowTabbing:Bool::NO] };

        // Custom positioned window buttons are only supported with a transparent or hidden titlebar
        let has_custom_titlebar = builder.macos_titlebar_style == MacosTitlebarStyle::Transparent
            || builder.macos_titlebar_style == MacosTitlebarStyle::Hidden;
        let traffic_light_position = builder
            .macos_traffic_light_position
            .filter(|_| has_custom_titlebar)
            .map(|position| NSPoint::new(position.x.into(), position.y.into()));

        // Create WindowDelegate instance
        let delegate: Allocated<WindowDelegate> =
            unsafe { msg_send![WindowDelegate::class(), alloc] };
        let window_delegate: Retained<Object> = unsafe {
            msg_send![
                super(delegate.set_ivars(WindowDelegateIvars {
                    host: host.clone(),
                    traffic_light_position,
                })),
                init
            ]
        };

        // Create window
        let screen_rect: NSRect = if let Some(monitor) = builder.monitor {
            unsafe { msg_send![&monitor.screen, frame] }
        } else {
            let screen: *mut Object = unsafe { msg_send![class!(NSScreen), mainScreen] };
            unsafe { msg_send![screen, frame] }
        };
        let window_rect = if builder.should_fullscreen {
            screen_rect
        } else {
            NSRect::new(
                if let Some(position) = builder.position {
                    NSPoint::new(
                        screen_rect.origin.x + position.x as f64,
                        screen_rect.origin.y
                            + (screen_rect.size.height - builder.size.height as f64)
                            - position.y as f64,
                    )
                } else {
                    NSPoint::new(
                        screen_rect.origin.x
                            + (screen_rect.size.width - builder.size.width as f64) / 2.0,
                        screen_rect.origin.y
                            + (screen_rect.size.height - builder.size.height as f64) / 2.0,
                    )
                },
                NSSize::new(builder.size.width as f64, builder.size.height as f64),
            )
        };

        let mut window_style_mask = NS_WINDOW_STYLE_MASK_TITLED
            | NS_WINDOW_STYLE_MASK_CLOSABLE
            | NS_WINDOW_STYLE_MASK_MINIATURIZABLE;
        if builder.resizable {
            window_style_mask |= NS_WINDOW_STYLE_MASK_RESIZABLE;
        }
        if builder.macos_titlebar_style == MacosTitlebarStyle::Transparent
            || builder.macos_titlebar_style == MacosTitlebarStyle::Hidden
        {
            window_style_mask |= NS_WINDOW_STYLE_MASK_FULL_SIZE_CONTENT_VIEW;
        }
        if builder.should_fullscreen {
            window_style_mask = 0;
        }

        let window = unsafe {
            let window: Allocated<Object> = msg_send![class!(NSWindow), alloc];
            let window: Retained<Object> = msg_send![window, initWithContentRect:NSRect::new(NSPoint::new(0.0, 0.0), window_rect.size),
                styleMask:window_style_mask, backing:NS_BACKING_STORE_BUFFERED, defer:Bool::NO];
            let _: () = msg_send![&window, setReleasedWhenClosed:Bool::NO];
            let _: () = msg_send![&window, setFrameOrigin:window_rect.origin];
            let _: () = msg_send![&window, setTitle:&*NSString::new(&builder.title)];
            if builder.should_fullscreen {
                let _: () = msg_send![&window, setLevel: 25i64];
            }
            if let Some(color) = builder.background_color {
                let color: *mut Object = msg_send![class!(NSColor), colorWithRed:((color >> 16) & 0xFF) as f64 / 255.0,
                    green:((color >> 8) & 0xFF) as f64 / 255.0,
                    blue:(color & 0xFF) as f64 / 255.0, alpha:1.0];
                let _: () = msg_send![&window, setBackgroundColor:color];
            }
            if builder.macos_titlebar_style == MacosTitlebarStyle::Transparent
                || builder.macos_titlebar_style == MacosTitlebarStyle::Hidden
            {
                let _: () = msg_send![&window, setTitlebarAppearsTransparent:Bool::YES];
            }
            if builder.macos_titlebar_style == MacosTitlebarStyle::Hidden {
                let _: () =
                    msg_send![&window, setTitleVisibility:NS_WINDOW_TITLE_VISIBILITY_HIDDEN];
            }
            if let Some(theme) = builder.theme {
                let appearance: *mut Object = msg_send![class!(NSAppearance), appearanceNamed:match theme {
                    Theme::Light => NSAppearanceNameAqua,
                    Theme::Dark => NSAppearanceNameDarkAqua,
                }];
                let _: () = msg_send![&window, setAppearance:appearance];
            }
            if let Some(min_size) = builder.min_size {
                let _: () = msg_send![&window, setContentMinSize:NSSize::new(min_size.width as f64, min_size.height as f64)];
            }
            #[cfg(feature = "remember_window_state")]
            if builder.remember_window_state {
                let _: Bool = msg_send![&window, setFrameAutosaveName:ns_string!("window")];
            }
            let _: () = msg_send![&window, setDelegate:window_delegate.as_ptr()];
            #[cfg(feature = "file_drop")]
            if builder.allow_file_drop {
                register_dragged_types(window.as_ptr());
            }
            window
        };

        // The shared content view renderers draw into, it receives the interaction.
        let content_view = unsafe {
            let initial: *mut Object = msg_send![&window, contentView];
            let frame: NSRect = msg_send![initial, frame];
            let content_view = ContentView::new(
                NSRect::new(NSPoint::new(0.0, 0.0), frame.size),
                host.event_sender(),
            );
            let _: () = msg_send![&window, setContentView:&*content_view];
            let _: Bool = msg_send![&window, makeFirstResponder:&*content_view];
            let _: () = msg_send![&window, setAcceptsMouseMovedEvents:Bool::YES];
            let scale: f64 = msg_send![&window, backingScaleFactor];
            host.scale_factor_changed(scale as f32);
            content_view
        };

        if !builder.should_fullscreen && has_custom_titlebar {
            // With custom positioned window buttons the content draws the titlebar and starts window drags
            if let Some(position) = traffic_light_position {
                layout_window_buttons(window.as_ptr(), position);
            } else {
                add_drag_view(window.as_ptr(), content_view.as_ptr().cast());
            }
        }
        host.set_handle(crate::NativeWindowHandle::AppKit(window.as_ptr().cast()));
        host.set_content(crate::NativeContentHandle::AppKit(
            content_view.as_ptr().cast(),
        ));
        unsafe {
            let _: () = msg_send![&window, addObserver:window_delegate.as_ptr(), forKeyPath:ns_string!("effectiveAppearance"), options:5usize, context:null_mut::<std::ffi::c_void>()];
        }
        // Windows built before the launch finished are shown by it, later ones right away.
        if has_launched() {
            let _: () = unsafe { msg_send![&window, makeKeyAndOrderFront:null_mut::<Object>()] };
            send_event(crate::Event::Window(builder.window_id, WindowEvent::Create));
        }
        PlatformWindow(Box::new(PlatformWindowData {
            window_id: builder.window_id,
            window,
            content_view,
            _delegate: window_delegate,
            background_color: builder.background_color,
            traffic_light_position,
        }))
    }
}

impl Drop for PlatformWindow {
    fn drop(&mut self) {
        let key = NSString::new("effectiveAppearance");
        let _: () = unsafe {
            msg_send![&self.0.window, removeObserver:self.0._delegate.as_ptr(), forKeyPath:&*key]
        };
        // NSWindow.delegate is non-owning, so clear it before the retained delegate is dropped.
        let _: () = unsafe { msg_send![&self.0.window, setDelegate:null_mut::<Object>()] };
    }
}

impl crate::WindowInterface for PlatformWindow {
    fn close(&mut self) {
        allow_termination_if_last_window(self.0.window.as_ptr());
        let _: () = unsafe { msg_send![&self.0.window, close] };
    }

    fn set_title(&mut self, title: impl AsRef<str>) {
        let _: () = unsafe { msg_send![&self.0.window, setTitle:&*NSString::new(title)] };
        if let Some(position) = self.0.traffic_light_position {
            layout_window_buttons(self.0.window.as_ptr(), position);
        }
    }

    fn position(&self) -> LogicalPoint {
        let frame: NSRect = unsafe { msg_send![&self.0.window, frame] };
        LogicalPoint::new(frame.origin.x as f32, frame.origin.y as f32)
    }

    fn size(&self) -> LogicalSize {
        let content_view: *mut Object = unsafe { msg_send![&self.0.window, contentView] };
        let frame: NSRect = unsafe { msg_send![content_view, frame] };
        LogicalSize::new(frame.size.width as f32, frame.size.height as f32)
    }

    fn set_position(&mut self, point: LogicalPoint) {
        unsafe {
            msg_send![&self.0.window, setFrameTopLeftPoint:NSPoint::new(point.x as f64, point.y as f64)]
        }
    }

    fn set_size(&mut self, size: LogicalSize) {
        unsafe {
            msg_send![&self.0.window, setContentSize:NSSize::new(size.width as f64, size.height as f64)]
        }
    }

    fn set_min_size(&mut self, min_size: LogicalSize) {
        unsafe {
            msg_send![&self.0.window, setContentMinSize:NSSize::new(min_size.width as f64, min_size.height as f64)]
        }
    }

    fn set_resizable(&mut self, resizable: bool) {
        let mut style_mask: u64 = unsafe { msg_send![&self.0.window, styleMask] };
        if resizable {
            style_mask |= NS_WINDOW_STYLE_MASK_RESIZABLE;
        } else {
            style_mask &= !NS_WINDOW_STYLE_MASK_RESIZABLE;
        }
        unsafe { msg_send![&self.0.window, setStyleMask:style_mask] }
    }

    fn set_theme(&mut self, theme: Theme) {
        unsafe {
            let appearance: *mut Object = msg_send![class!(NSAppearance), appearanceNamed:match theme {
                Theme::Light => NSAppearanceNameAqua,
                Theme::Dark => NSAppearanceNameDarkAqua,
            }];
            let _: () = msg_send![&self.0.window, setAppearance:appearance];
        }
    }

    fn set_background_color(&mut self, color: u32) {
        self.0.background_color = Some(color);
        unsafe {
            let color_obj: *mut Object = msg_send![class!(NSColor), colorWithRed:((color >> 16) & 0xFF) as f64 / 255.0,
                green:((color >> 8) & 0xFF) as f64 / 255.0,
                blue:(color & 0xFF) as f64 / 255.0, alpha:1.0];
            let _: () = msg_send![&self.0.window, setBackgroundColor:color_obj];
        }
    }

    fn set_cursor(&mut self, cursor: CursorIcon) {
        self.0.content_view.set_cursor(cursor);
    }

    fn request_pointer_lock(&mut self) -> Result<(), PointerLockError> {
        let status = unsafe { CGAssociateMouseAndMouseCursorPosition(0) };
        if status != 0 {
            return Err(PointerLockError(format!(
                "Core Graphics pointer lock failed with status {status}"
            )));
        }
        let _: () = unsafe { msg_send![class!(NSCursor), hide] };
        Ok(())
    }

    fn exit_pointer_lock(&mut self) {
        release_pointer_lock();
    }

    fn macos_titlebar_size(&self) -> LogicalSize {
        let window_frame: NSRect = unsafe { msg_send![&self.0.window, frame] };
        if let Some(titlebar_height) = self
            .0
            .traffic_light_position
            .and_then(|position| traffic_light_titlebar_height(self.0.window.as_ptr(), position))
        {
            return LogicalSize::new(window_frame.size.width as f32, titlebar_height as f32);
        }
        let content_layout_rect: NSRect = unsafe { msg_send![&self.0.window, contentLayoutRect] };
        LogicalSize::new(
            window_frame.size.width as f32,
            (window_frame.size.height - content_layout_rect.size.height) as f32,
        )
    }

    fn macos_set_document_edited(&mut self, edited: bool) {
        let _: () = unsafe { msg_send![&self.0.window, setDocumentEdited:edited] };
    }

    fn macos_start_window_drag(&mut self) {
        let event: *mut Object = unsafe { msg_send![NSApp, currentEvent] };
        if event.is_null() {
            return;
        }
        let event_type: u64 = unsafe { msg_send![event, type] };
        if event_type == NS_EVENT_TYPE_LEFT_MOUSE_DOWN
            || event_type == NS_EVENT_TYPE_LEFT_MOUSE_DRAGGED
        {
            let _: () = unsafe { msg_send![&self.0.window, performWindowDragWithEvent:event] };
        }
    }

    fn macos_perform_titlebar_double_click(&mut self) {
        perform_titlebar_double_click(self.0.window.as_ptr());
    }
}

/// Release the pointer lock of any window.
pub(crate) fn release_pointer_lock() {
    let _ = unsafe { CGAssociateMouseAndMouseCursorPosition(1) };
    let _: () = unsafe { msg_send![class!(NSCursor), unhide] };
}

pub(super) fn window_id(window: *mut Object) -> Option<crate::WindowId> {
    if window.is_null() {
        return None;
    }
    let delegate: *mut Object = unsafe { msg_send![window, delegate] };
    if delegate.is_null() {
        return None;
    }
    let class = WindowDelegate::class().cast::<objc2::runtime::AnyClass>();
    let is_ours: Bool = unsafe { msg_send![delegate, isKindOfClass:class] };
    if is_ours == Bool::NO {
        return None;
    }
    let host = &unsafe { &*delegate.cast::<WindowDelegate>() }.ivars().host;
    (!host.is_closed()).then(|| host.id())
}

#[cfg(test)]
mod tests {
    use super::TitlebarDoubleClickAction;

    #[test]
    fn parses_titlebar_double_click_preferences() {
        assert_eq!(
            TitlebarDoubleClickAction::from_preference(Some("Minimize")),
            TitlebarDoubleClickAction::Minimize
        );
        assert_eq!(
            TitlebarDoubleClickAction::from_preference(Some("None")),
            TitlebarDoubleClickAction::None
        );
        assert_eq!(
            TitlebarDoubleClickAction::from_preference(Some("Maximize")),
            TitlebarDoubleClickAction::Zoom
        );
        assert_eq!(
            TitlebarDoubleClickAction::from_preference(Some("Fill")),
            TitlebarDoubleClickAction::Zoom
        );
        assert_eq!(
            TitlebarDoubleClickAction::from_preference(None),
            TitlebarDoubleClickAction::Zoom
        );
    }
}
