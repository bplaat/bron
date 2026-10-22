/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Vulkan surfaces for bwindow GTK content widgets.

use std::cell::{Cell, RefCell};
use std::ffi::{c_ulong, c_void};
use std::ptr;
use std::rc::Rc;

use super::surface::{SurfaceOwner, Swapchain};
use super::*;
use crate::platform::headers::{
    gdk_wayland_display_get_wl_display, gdk_wayland_window_get_wl_surface, gdk_window_get_display,
    gdk_x11_display_get_xdisplay, gdk_x11_window_get_xid, gtk_widget_add_tick_callback,
    gtk_widget_get_scale_factor, gtk_widget_get_window, gtk_widget_remove_tick_callback,
};

pub(super) enum PlatformWindow {
    Xlib {
        display: *mut c_void,
        window: c_ulong,
    },
    Wayland {
        display: *mut c_void,
        surface: *mut c_void,
    },
}

pub(super) fn platform_window(handle: bwindow::NativeWindowHandle) -> Result<PlatformWindow> {
    let bwindow::NativeWindowHandle::Gtk(widget) = handle else {
        return Err(Error("Vulkan requires a GTK window".into()));
    };
    unsafe {
        let window = gtk_widget_get_window(widget);
        if window.is_null() {
            return Err(Error("GTK window is not realized".into()));
        }
        let display = gdk_window_get_display(window);
        if display.is_null() {
            return Err(Error("GTK window has no display".into()));
        }
        if vk::g_type_check_instance_is_a(display, vk::gdk_wayland_display_get_type()) != 0 {
            let display = gdk_wayland_display_get_wl_display(display);
            let surface = gdk_wayland_window_get_wl_surface(window);
            if display.is_null() || surface.is_null() {
                return Err(Error("GTK returned invalid Wayland handles".into()));
            }
            Ok(PlatformWindow::Wayland { display, surface })
        } else {
            let display = gdk_x11_display_get_xdisplay(display);
            let window = gdk_x11_window_get_xid(window);
            if display.is_null() || window == 0 {
                return Err(Error("GTK returned invalid X11 handles".into()));
            }
            Ok(PlatformWindow::Xlib { display, window })
        }
    }
}

pub(crate) struct Surface {
    pub(super) instance: Rc<Instance>,
    owner: Rc<SurfaceOwner>,
    pub(super) swap: RefCell<Option<Rc<Swapchain>>>,
    content: Rc<widget::Widget>,
    pub(super) widget: *mut c_void,
    pub(super) redraw: Rc<RedrawTimer>,
    pub(super) startup_clock: *mut c_void,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            if !self.startup_clock.is_null() {
                vk::g_signal_handlers_disconnect_matched(
                    self.startup_clock.cast(),
                    16, // G_SIGNAL_MATCH_DATA
                    0,
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    Rc::as_ptr(&self.redraw) as *mut c_void,
                );
                vk::g_object_unref(self.startup_clock.cast());
            }
            let window = gtk_widget_get_window(self.widget);
            if !window.is_null() {
                vk::gdk_window_set_invalidate_handler(window, None);
                vk::g_object_set_data(window, c"bplaat-bwebgpu-redraw".as_ptr(), ptr::null_mut());
            }
            vk::g_signal_handlers_disconnect_matched(
                self.widget.cast(),
                16, // G_SIGNAL_MATCH_DATA
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                Rc::as_ptr(&self.redraw) as *mut c_void,
            );
        }
        if let Some(id) = self.redraw.callback.take() {
            unsafe { gtk_widget_remove_tick_callback(self.widget, id) };
        }
    }
}

pub(super) struct RedrawTimer {
    widget: *mut c_void,
    sender: bwindow::WindowEventSender,
    pub(super) callback: Cell<Option<u32>>,
    ready: Cell<bool>,
}

extern "C" fn surface_after_paint(_: *mut c_void, data: *const RedrawTimer) {
    let timer = unsafe {
        Rc::increment_strong_count(data);
        Rc::from_raw(data)
    };
    if !timer.ready.replace(true) {
        timer.request_animation_frame();
    }
}

impl Drop for RedrawTimer {
    fn drop(&mut self) {
        if let Some(id) = self.callback.take() {
            unsafe { gtk_widget_remove_tick_callback(self.widget, id) };
        }
    }
}

impl RedrawTimer {
    fn request_animation_frame(self: &Rc<Self>) {
        if self.callback.get().is_some() {
            return;
        }
        let data = Rc::into_raw(self.clone()) as *mut c_void;
        let id =
            unsafe { gtk_widget_add_tick_callback(self.widget, redraw_tick, data, release_redraw) };
        self.callback.set(Some(id));
    }
}

extern "C" fn surface_invalidate(window: *mut c_void, region: *mut c_void) {
    unsafe {
        // This native window belongs to Vulkan. Preserve the repaint request,
        // but remove its Cairo damage so GDK cannot attach a competing buffer.
        let pointer =
            vk::g_object_get_data(window, c"bplaat-bwebgpu-redraw".as_ptr()).cast::<RedrawTimer>();
        if !pointer.is_null() {
            Rc::increment_strong_count(pointer);
            let timer = Rc::from_raw(pointer);
            timer.request_animation_frame();
        }
        vk::cairo_region_subtract(region, region);
    }
}

extern "C" fn surface_draw(_: *mut c_void, _: *mut c_void, data: *const RedrawTimer) -> i32 {
    // GTK flushes its backing buffer after this signal, which can overwrite a
    // Vulkan presentation. Repaint on the next tick, after GTK finishes drawing.
    let timer = unsafe {
        Rc::increment_strong_count(data);
        Rc::from_raw(data)
    };
    timer.request_animation_frame();
    1
}

extern "C" fn redraw_tick(_: *mut c_void, _: *mut c_void, data: *mut c_void) -> i32 {
    // The application can close the window (removing this callback) while
    // handling the redraw. Retain the callback data across that delivery.
    let timer = unsafe {
        let pointer = data.cast::<RedrawTimer>();
        Rc::increment_strong_count(pointer);
        Rc::from_raw(pointer)
    };
    timer.callback.set(None);
    timer.sender.send(bwindow::WindowEvent::RedrawRequested);
    0
}

extern "C" fn release_redraw(data: *mut c_void) {
    drop(unsafe { Rc::from_raw(data.cast::<RedrawTimer>()) });
}

impl Surface {
    pub(crate) fn new(
        attachment: &bwindow::WindowAttachment,
        factory: &mut Option<Factory>,
    ) -> Result<Self> {
        unsafe {
            let sender = attachment.event_sender();
            let (
                Some(bwindow::NativeWindowHandle::Gtk(parent)),
                Some(bwindow::NativeContentHandle::Gtk(area)),
            ) = (attachment.window_handle(), attachment.content_handle())
            else {
                return Err(Error("Vulkan requires a GTK window".into()));
            };
            let content = widget::Widget::new(area, parent);
            let widget = content.handle;
            let window = platform_window(bwindow::NativeWindowHandle::Gtk(widget))?;
            let wayland = matches!(window, PlatformWindow::Wayland { .. });
            let platform = match window {
                PlatformWindow::Xlib { .. } => c"VK_KHR_xlib_surface",
                PlatformWindow::Wayland { .. } => c"VK_KHR_wayland_surface",
            };
            let instance = if let Some(instance) = factory {
                instance.clone()
            } else {
                let instance = Instance::new(Some(platform))?;
                *factory = Some(instance.clone());
                instance
            };
            let handle = instance.handle;
            let mut surface = 0;
            match window {
                PlatformWindow::Xlib { display, window } => {
                    let info = vk::XlibSurfaceCreateInfoKHR {
                        s_type: 1000004000,
                        dpy: display,
                        window,
                        ..Default::default()
                    };
                    check(call!(
                        instance,
                        CreateXlibSurfaceKHR,
                        handle,
                        &info,
                        ptr::null(),
                        &mut surface
                    ))?;
                }
                PlatformWindow::Wayland {
                    display,
                    surface: window,
                } => {
                    let info = vk::WaylandSurfaceCreateInfoKHR {
                        s_type: 1000006000,
                        display,
                        surface: window,
                        ..Default::default()
                    };
                    check(call!(
                        instance,
                        CreateWaylandSurfaceKHR,
                        handle,
                        &info,
                        ptr::null(),
                        &mut surface
                    ))?;
                }
            }
            let redraw = Rc::new(RedrawTimer {
                widget,
                sender,
                callback: Cell::new(None),
                ready: Cell::new(!wayland),
            });
            // GDK keeps a new Wayland subsurface synchronized until the parent
            // commits its first frame. Presenting repeatedly before that commit
            // can merge Vulkan FIFO timestamps and cause a fatal protocol error.
            let startup_clock = if wayland {
                let clock = vk::gtk_widget_get_frame_clock(parent);
                vk::g_object_ref(clock.cast());
                vk::g_signal_connect_data(
                    clock.cast(),
                    c"after-paint".as_ptr(),
                    surface_after_paint as *const c_void,
                    Rc::as_ptr(&redraw).cast(),
                    ptr::null(),
                    1, // G_CONNECT_AFTER: GDK must commit and desynchronize first.
                );
                clock
            } else {
                ptr::null_mut()
            };
            let native_window = gtk_widget_get_window(widget);
            vk::g_object_set_data(
                native_window,
                c"bplaat-bwebgpu-redraw".as_ptr(),
                Rc::as_ptr(&redraw) as *mut c_void,
            );
            vk::gdk_window_set_invalidate_handler(native_window, Some(surface_invalidate));
            // Discard Cairo damage queued while GTK realized and mapped the widget.
            let pending = vk::gdk_window_get_update_area(native_window);
            if !pending.is_null() {
                vk::cairo_region_destroy(pending);
            }
            vk::g_signal_connect_data(
                widget.cast(),
                c"draw".as_ptr(),
                surface_draw as *const c_void,
                Rc::as_ptr(&redraw).cast(),
                ptr::null(),
                vk::G_CONNECT_DEFAULT,
            );
            Ok(Self {
                owner: Rc::new(SurfaceOwner {
                    instance: instance.clone(),
                    handle: surface,
                    _window: Some(content.clone()),
                }),
                instance,
                swap: RefCell::new(None),
                content,
                widget,
                redraw,
                startup_clock,
            })
        }
    }

    pub(crate) fn request_redraw(&self) {
        self.redraw
            .sender
            .send(bwindow::WindowEvent::RedrawRequested);
    }

    pub(crate) fn request_animation_frame(&self) {
        self.redraw.request_animation_frame();
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        if self.is_lost() {
            return (0, 0);
        }
        let width = unsafe { vk::gtk_widget_get_allocated_width(self.widget) };
        let height = unsafe { vk::gtk_widget_get_allocated_height(self.widget) };
        let scale = unsafe { gtk_widget_get_scale_factor(self.widget) }.max(1);
        (
            width.max(0).saturating_mul(scale) as u32,
            height.max(0).saturating_mul(scale) as u32,
        )
    }

    pub(super) fn is_lost(&self) -> bool {
        self.content.lost.get() || unsafe { gtk_widget_get_window(self.widget) }.is_null()
    }

    /// GDK keeps a new Wayland subsurface synchronized until the parent commits a frame.
    pub(super) fn is_ready(&self) -> bool {
        self.redraw.ready.get()
    }

    /// The Vulkan surface, which lives as long as the content widget.
    pub(super) fn owner(&self) -> Option<Rc<SurfaceOwner>> {
        Some(self.owner.clone())
    }
}
