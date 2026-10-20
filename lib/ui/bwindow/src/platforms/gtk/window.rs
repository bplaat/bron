/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CString, c_void};
use std::ptr::{null, null_mut};

use super::event_loop::{primary_monitor_rect, send_event};
use super::headers::*;
#[cfg(feature = "progress_bar")]
use super::progress_bar::update_progress_bar;
#[cfg(feature = "remember_window_state")]
use super::window_state::{load_window_state, save_window_state};
use crate::{CloseRequest, LogicalPoint, LogicalSize, Theme, WindowBuilder, WindowEvent};

pub(super) struct WindowData {
    host: std::rc::Rc<crate::content::ContentHost>,
    pub(crate) window_id: crate::WindowId,
    pub(super) window: *mut GtkWindow,
    pub(super) background_color: Option<u32>,
    #[cfg(feature = "remember_window_state")]
    pub(super) remember_window_state: bool,
    #[cfg(feature = "file_drop")]
    pub(super) allow_file_drop: bool,
}

extern "C" fn window_on_destroy(_: *mut GtkWidget, data: &WindowData) {
    data.host.close();
}

extern "C" fn window_on_style_updated(_: *mut GtkWidget, data: &WindowData) {
    data.host.theme_changed(super::event_loop::system_theme());
}

pub(crate) struct PlatformWindow(pub(super) Box<WindowData>);

impl PlatformWindow {
    pub(crate) const fn id(&self) -> crate::WindowId {
        self.0.window_id
    }

    pub(crate) fn new(
        builder: &WindowBuilder,
        host: std::rc::Rc<crate::content::ContentHost>,
    ) -> Self {
        let is_wayland = unsafe { gdk_display_is_wayland(gdk_display_get_default()) };

        let settings = unsafe { gtk_settings_get_default() };

        // Apply an explicit application theme preference
        if let Some(theme) = builder.theme {
            unsafe {
                g_object_set(
                    settings as *mut GObject,
                    c"gtk-application-prefer-dark-theme".as_ptr(),
                    if theme == Theme::Dark { 1 } else { 0 } as *const c_void,
                    null::<c_void>(),
                );
            }
        }

        let mut window_data = Box::new(WindowData {
            host: host.clone(),
            window_id: builder.window_id,
            window: null_mut(),
            background_color: builder.background_color,
            #[cfg(feature = "remember_window_state")]
            remember_window_state: builder.remember_window_state,
            #[cfg(feature = "file_drop")]
            allow_file_drop: builder.allow_file_drop,
        });

        // Create window
        let window = unsafe {
            let window = gtk_application_window_new(super::event_loop::application());
            gtk_application_window_set_show_menubar(window, 0);
            g_object_ref(window.cast());
            #[cfg(feature = "file_drop")]
            if builder.allow_file_drop {
                super::file_drop::gtk_connect_file_drop(window.cast(), host.event_sender());
            }
            let title = CString::new(builder.title.clone()).expect("Can't convert to CString");
            gtk_window_set_title(window, title.as_ptr());
            gtk_window_set_default_size(
                window,
                builder.size.width as i32,
                builder.size.height as i32,
            );
            gtk_window_set_resizable(window, builder.resizable);
            if let Some(min_size) = builder.min_size {
                gtk_widget_set_size_request(
                    window as *mut GtkWidget,
                    min_size.width as i32,
                    min_size.height as i32,
                );
            }
            if let Some(color) = builder.background_color {
                let rgba = GdkRGBA {
                    red: ((color >> 16) & 0xFF) as f64 / 255.0,
                    green: ((color >> 8) & 0xFF) as f64 / 255.0,
                    blue: (color & 0xFF) as f64 / 255.0,
                    alpha: 1.0,
                };
                gtk_widget_override_background_color(
                    window as *mut GtkWidget,
                    GTK_STATE_FLAG_NORMAL,
                    &rgba,
                );
            }

            let monitor_rect = if let Some(monitor) = builder.monitor {
                monitor.rect()
            } else {
                primary_monitor_rect()
            };
            if let Some(position) = builder.position {
                gtk_window_move(
                    window,
                    position.x as i32 + monitor_rect.x,
                    position.y as i32 + monitor_rect.y,
                );
            }
            if builder.should_fullscreen {
                gtk_window_move(window, monitor_rect.x, monitor_rect.y);
                gtk_window_fullscreen(window);
            }
            if builder.should_center {
                if !is_wayland {
                    gtk_window_move(
                        window,
                        monitor_rect.x + (monitor_rect.width - builder.size.width as i32) / 2,
                        monitor_rect.y + (monitor_rect.height - builder.size.height as i32) / 2,
                    );
                } else {
                    gtk_window_set_position(window, GTK_WIN_POS_CENTER);
                }
            }
            #[cfg(feature = "remember_window_state")]
            if builder.remember_window_state {
                load_window_state(window);
            }

            g_signal_connect_data(
                window as *mut GObject,
                c"destroy".as_ptr(),
                window_on_destroy as *const c_void,
                window_data.as_mut() as *mut _ as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            g_signal_connect_data(
                window as *mut GObject,
                c"style-updated".as_ptr(),
                window_on_style_updated as *const c_void,
                window_data.as_mut() as *mut _ as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            if !is_wayland {
                g_signal_connect_data(
                    window as *mut GObject,
                    c"configure-event".as_ptr(),
                    window_on_move as *const c_void,
                    window_data.as_mut() as *mut _ as *const c_void,
                    null(),
                    G_CONNECT_DEFAULT,
                );
            }
            g_signal_connect_data(
                window as *mut GObject,
                c"size-allocate".as_ptr(),
                window_on_resize as *const c_void,
                window_data.as_mut() as *mut _ as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            g_signal_connect_data(
                window.cast(),
                c"notify::scale-factor".as_ptr(),
                window_on_scale_changed as *const c_void,
                window_data.as_mut() as *mut _ as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            g_signal_connect_data(
                window as *mut GObject,
                c"delete-event".as_ptr(),
                window_on_close as *const c_void,
                window_data.as_mut() as *mut _ as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            window
        };

        window_data.window = window;
        unsafe {
            super::menu::connect_window(window);
        }
        host.set_handle(crate::NativeWindowHandle::Gtk(window.cast()));
        unsafe { gtk_widget_show_all(window.cast()) };
        send_event(crate::Event::Window(builder.window_id, WindowEvent::Create));
        PlatformWindow(window_data)
    }
}

impl crate::WindowInterface for PlatformWindow {
    fn close(&mut self) {
        #[cfg(feature = "remember_window_state")]
        if self.0.remember_window_state {
            save_window_state(self.0.window);
        }
        unsafe { gtk_widget_destroy(self.0.window as *mut GtkWidget) };
    }

    fn set_title(&mut self, title: impl AsRef<str>) {
        let title = CString::new(title.as_ref()).expect("Can't convert to CString");
        unsafe { gtk_window_set_title(self.0.window, title.as_ptr()) };
    }

    fn position(&self) -> LogicalPoint {
        let mut x = 0;
        let mut y = 0;
        unsafe { gtk_window_get_position(self.0.window, &mut x, &mut y) };
        LogicalPoint::new(x as f32, y as f32)
    }

    fn size(&self) -> LogicalSize {
        let mut width = 0;
        let mut height = 0;
        unsafe { gtk_window_get_size(self.0.window, &mut width, &mut height) };
        LogicalSize::new(width as f32, height as f32)
    }

    fn set_position(&mut self, point: LogicalPoint) {
        let primary_monitor_rect = primary_monitor_rect();
        unsafe {
            gtk_window_move(
                self.0.window,
                point.x as i32 + primary_monitor_rect.x,
                point.y as i32 + primary_monitor_rect.y,
            )
        }
    }

    fn set_size(&mut self, size: LogicalSize) {
        unsafe { gtk_window_set_default_size(self.0.window, size.width as i32, size.height as i32) }
    }

    fn set_min_size(&mut self, min_size: LogicalSize) {
        unsafe {
            gtk_widget_set_size_request(
                self.0.window as *mut GtkWidget,
                min_size.width as i32,
                min_size.height as i32,
            )
        }
    }

    fn set_resizable(&mut self, resizable: bool) {
        unsafe { gtk_window_set_resizable(self.0.window, resizable) }
    }

    fn set_theme(&mut self, theme: Theme) {
        unsafe {
            let settings = gtk_settings_get_default();
            g_object_set(
                settings as *mut GObject,
                c"gtk-application-prefer-dark-theme".as_ptr(),
                if theme == Theme::Dark { 1 } else { 0 } as *const c_void,
                null::<c_void>(),
            );
        }
    }

    fn set_background_color(&mut self, color: u32) {
        self.0.background_color = Some(color);
        unsafe {
            let rgba = GdkRGBA {
                red: ((color >> 16) & 0xFF) as f64 / 255.0,
                green: ((color >> 8) & 0xFF) as f64 / 255.0,
                blue: (color & 0xFF) as f64 / 255.0,
                alpha: 1.0,
            };
            gtk_widget_override_background_color(
                self.0.window as *mut GtkWidget,
                GTK_STATE_FLAG_NORMAL,
                &rgba,
            );
        }
    }

    #[cfg(feature = "progress_bar")]
    fn gtk_set_progress_bar(&mut self, progress: Option<f32>) {
        update_progress_bar(progress);
    }
}

extern "C" fn window_on_move(
    _window: *mut GtkWindow,
    _allocation: *mut c_void,
    _self: &mut WindowData,
) -> bool {
    let mut x = 0;
    let mut y = 0;
    unsafe { gtk_window_get_position(_self.window, &mut x, &mut y) };
    send_event(crate::Event::Window(
        _self.window_id,
        WindowEvent::Move(LogicalPoint::new(x as f32, y as f32)),
    ));
    false
}

extern "C" fn window_on_resize(
    _window: *mut GtkWindow,
    _allocation: *mut c_void,
    _self: &mut WindowData,
) {
    let mut width = 0;
    let mut height = 0;
    unsafe { gtk_window_get_size(_self.window, &mut width, &mut height) };
    let scale = unsafe { gtk_widget_get_scale_factor(_self.window.cast()) };
    let host = _self.host.clone();
    let id = _self.window_id;
    host.resize((width * scale) as u32, (height * scale) as u32);
    if host.is_closed() {
        return;
    }
    send_event(crate::Event::Window(
        id,
        WindowEvent::Resize(LogicalSize::new(width as f32, height as f32)),
    ));
}

extern "C" fn window_on_close(
    _window: *mut GtkWindow,
    _event: *mut c_void,
    _self: &mut WindowData,
) -> bool {
    let request = CloseRequest::new();
    let window = _self.window;
    let host = _self.host.clone();
    #[cfg(feature = "remember_window_state")]
    let remember = _self.remember_window_state;
    // Keep the GObject alive if this request is queued by a nested native callback.
    unsafe { g_object_ref(window as *mut GObject) };
    crate::dispatch::send_then(
        crate::Event::Window(
            _self.window_id,
            WindowEvent::CloseRequested(request.clone()),
        ),
        move || {
            if !request.default_prevented() && !host.is_closed() {
                host.close();
                #[cfg(feature = "remember_window_state")]
                if remember {
                    save_window_state(window);
                }
                unsafe { gtk_widget_destroy(window as *mut GtkWidget) };
            }
            unsafe { g_object_unref(window as *mut GObject) };
        },
    );
    true
}

extern "C" fn window_on_scale_changed(window: *mut GtkWidget, _: *mut c_void, data: &WindowData) {
    let host = data.host.clone();
    if host.is_closed() {
        return;
    }
    let scale = unsafe { gtk_widget_get_scale_factor(window) };
    let width = unsafe { gtk_widget_get_allocated_width(window) };
    let height = unsafe { gtk_widget_get_allocated_height(window) };
    host.resize((width * scale) as u32, (height * scale) as u32);
    if !host.is_closed() {
        send_event(crate::Event::Window(
            host.id(),
            WindowEvent::ScaleFactorChanged(scale as f32),
        ));
        host.request_redraw();
    }
}

impl Drop for PlatformWindow {
    fn drop(&mut self) {
        unsafe {
            g_signal_handlers_disconnect_matched(
                self.0.window.cast(),
                16,
                0,
                0,
                null_mut(),
                null_mut(),
                self.0.as_mut() as *mut _ as *mut c_void,
            );
            g_object_unref(self.0.window.cast());
        }
    }
}

#[cfg(all(test, feature = "menu"))]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::{
        Event, EventLoopBuilder, EventLoopInterface, EventLoopProxyInterface, MenuBarBuilder,
        MenuBuilder, MenuItem,
    };

    #[test]
    #[ignore = "requires a display and session bus"]
    fn application_menu_exports_defaults_resizes_content_and_preserves_quit_cancellation() {
        let event_loop = super::super::event_loop::PlatformEventLoop::new(
            EventLoopBuilder::new().menu(
                MenuBarBuilder::new()
                    .menu(MenuBuilder::new("File").item(MenuItem::new("New", "bwindow-quit"))),
            ),
        );
        let app = super::super::event_loop::application().cast();
        unsafe {
            assert_eq!(
                g_action_group_get_action_enabled(app, c"bwindow-close".as_ptr()),
                0
            );
            assert_eq!(
                g_action_group_get_action_enabled(app, c"bwindow-quit".as_ptr()),
                0
            );
        }
        let window = WindowBuilder::new().build();
        let native = window.platform.0.window;
        unsafe {
            assert_eq!(gtk_application_window_get_show_menubar(native), 0);
            let model = gtk_application_get_menubar(super::super::event_loop::application());
            assert!(!model.is_null());
            assert_eq!(g_menu_model_get_n_items(model), 5);
        }
        let content = window.attach_content().expect("empty window");
        let custom_count = Rc::new(Cell::new(0));
        let custom_actions = custom_count.clone();
        let undo_count = Rc::new(Cell::new(0u32));
        let undone = undo_count.clone();
        let resized = Rc::new(Cell::new(None));
        let result = resized.clone();
        content.on_resize(move |width, height| result.set(Some((width, height))));
        let seen = Rc::new(Cell::new(false));
        let result = seen.clone();
        let close_requests = Rc::new(Cell::new(0));
        let closed = close_requests.clone();
        let proxy = event_loop.create_proxy();
        assert!(proxy.send_user_event(Box::new(())));
        event_loop.run(move |event| {
            let _keep_alive = (&content, &window);
            match event {
                Event::UserEvent(_) => {
                    if closed.get() == 0 {
                        unsafe {
                            let entry = gtk_entry_new();
                            gtk_entry_set_text(entry, c"native selection".as_ptr());
                            gtk_container_add(native.cast(), entry);
                            gtk_widget_show(entry);
                            gtk_window_set_focus(native, entry);
                            let group = g_simple_action_group_new();
                            let action = g_simple_action_new(c"Undo".as_ptr(), null());
                            extern "C" fn undo(
                                _: *mut GSimpleAction,
                                _: *mut GVariant,
                                count: &Cell<u32>,
                            ) {
                                count.set(count.get() + 1);
                            }
                            g_signal_connect_data(
                                action.cast(),
                                c"activate".as_ptr(),
                                undo as *const c_void,
                                Rc::as_ptr(&undone).cast(),
                                null(),
                                G_CONNECT_DEFAULT,
                            );
                            g_simple_action_set_enabled(action, 0);
                            g_action_map_add_action(group, action);
                            gtk_widget_insert_action_group(entry, c"bwindow-edit".as_ptr(), group);
                            super::super::menu::update_actions();
                            let app = super::super::event_loop::application().cast();
                            gtk_window_set_resizable(native, false);
                            assert_eq!(
                                g_action_group_get_action_enabled(
                                    app,
                                    c"bwindow-maximize".as_ptr()
                                ),
                                0
                            );
                            gtk_window_set_resizable(native, true);
                            assert_ne!(
                                g_action_group_get_action_enabled(
                                    app,
                                    c"bwindow-maximize".as_ptr()
                                ),
                                0
                            );
                            g_action_group_activate_action(app, c"command0".as_ptr(), null_mut());
                            assert_eq!(
                                g_action_group_get_action_enabled(app, c"bwindow-undo".as_ptr()),
                                0
                            );
                            g_simple_action_set_enabled(action, 1);
                            assert_ne!(
                                g_action_group_get_action_enabled(app, c"bwindow-undo".as_ptr()),
                                0
                            );
                            g_action_group_activate_action(
                                app,
                                c"bwindow-undo".as_ptr(),
                                null_mut(),
                            );
                            g_object_unref(action.cast());
                            g_object_unref(group.cast());
                            super::super::menu::edit(native, c"SelectAll");
                            super::super::menu::edit(native, c"Cut");
                            assert_eq!(std::ffi::CStr::from_ptr(gtk_entry_get_text(entry)), c"");
                            gtk_widget_destroy(entry);
                        }
                    }
                    unsafe { g_object_notify(native.cast(), c"scale-factor".as_ptr()) };
                    unsafe {
                        g_action_group_activate_action(
                            super::super::event_loop::application().cast(),
                            c"bwindow-quit".as_ptr(),
                            null_mut(),
                        )
                    };
                }
                Event::Action(action) => {
                    assert_eq!(action, "bwindow-quit");
                    custom_actions.set(custom_actions.get() + 1);
                }
                Event::Window(_, WindowEvent::CloseRequested(request)) => {
                    let count = closed.get();
                    closed.set(count + 1);
                    if count == 0 {
                        request.prevent_default();
                        assert!(proxy.send_user_event(Box::new(())));
                    }
                }
                Event::Window(_, WindowEvent::ScaleFactorChanged(scale)) => {
                    let (width, height) = resized.get().expect("content was not resized");
                    assert!(width > 0 && height > 0 && scale >= 1.0);
                    result.set(true);
                }
                _ => {}
            }
        });
        assert!(seen.get(), "scale notification was not delivered");
        assert_eq!(close_requests.get(), 2, "Quit bypassed close cancellation");
        assert_eq!(undo_count.get(), 1, "focused widget action was not invoked");
        assert_eq!(
            custom_count.get(),
            1,
            "custom action collided with a default"
        );
    }
}
