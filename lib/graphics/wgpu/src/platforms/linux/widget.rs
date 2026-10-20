/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::c_void;
use std::ptr;
use std::rc::Rc;

use bwindow::ffi::{
    G_CONNECT_DEFAULT, GdkEvent, g_object_unref, g_signal_connect_data,
    g_signal_handlers_disconnect_matched, gdk_event_get_event_type, gdk_event_get_keycode,
    gtk_container_add, gtk_input_event, gtk_widget_destroy, gtk_widget_show,
};
use bwindow::{WindowEvent, WindowEventSender};

use super::headers::*;

pub(super) struct Widget {
    pub(super) handle: *mut c_void,
    sender: WindowEventSender,
    keys: RefCell<HashSet<u16>>,
    pub(super) lost: Cell<bool>,
}

impl Widget {
    pub(super) fn new(parent: *mut c_void, sender: WindowEventSender) -> Rc<Self> {
        unsafe {
            let handle = gtk_drawing_area_new();
            g_object_ref_sink(handle);
            gtk_widget_set_can_focus(handle, 1);
            gtk_widget_add_events(
                handle,
                (1 << 1)
                    | (1 << 2)
                    | (1 << 8)
                    | (1 << 9)
                    | (1 << 10)
                    | (1 << 11)
                    | (1 << 12)
                    | (1 << 13)
                    | (1 << 14)
                    | (1 << 21)
                    | (1 << 23),
            );
            let widget = Rc::new(Self {
                handle,
                sender,
                keys: RefCell::new(HashSet::new()),
                lost: Cell::new(false),
            });
            for (signal, callback) in [
                (c"realize", realize as *const c_void),
                (c"event", input as *const c_void),
                (c"focus-in-event", focus as *const c_void),
                (c"focus-out-event", blur as *const c_void),
                (c"unmap", unmap as *const c_void),
            ] {
                g_signal_connect_data(
                    handle.cast(),
                    signal.as_ptr(),
                    callback,
                    Rc::as_ptr(&widget).cast(),
                    ptr::null(),
                    G_CONNECT_DEFAULT,
                );
            }
            gtk_container_add(parent.cast(), handle.cast());
            // Allocate the content before creating the swapchain; GTK otherwise
            // leaves a newly attached drawing area at its initial 1x1 allocation.
            gtk_widget_show(handle.cast());
            gtk_container_check_resize(parent);
            gtk_widget_realize(handle);
            gtk_widget_grab_focus(handle);
            widget
        }
    }

    unsafe fn retain(pointer: *const Self) -> Rc<Self> {
        unsafe {
            Rc::increment_strong_count(pointer);
            Rc::from_raw(pointer)
        }
    }
}

impl Drop for Widget {
    fn drop(&mut self) {
        unsafe {
            g_signal_handlers_disconnect_matched(
                self.handle.cast(),
                16,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                self as *mut Self as *mut c_void,
            );
            gtk_widget_destroy(self.handle.cast());
            g_object_unref(self.handle.cast());
        }
    }
}

extern "C" fn realize(widget: *mut c_void, _: *const Widget) {
    unsafe {
        let window = gtk_widget_get_window(widget);
        let display = gdk_window_get_display(window);
        if g_type_check_instance_is_a(display, gdk_wayland_display_get_type()) != 0 {
            // GtkDrawingArea creates a non-native child on Wayland. Replace it
            // with a native subsurface so Vulkan never presents over GTK's CSD.
            let mut allocation = Allocation::default();
            gtk_widget_get_allocation(widget, &mut allocation);
            let attributes = WindowAttributes {
                event_mask: gtk_widget_get_events(widget) | (1 << 1),
                x: allocation.x,
                y: allocation.y,
                width: allocation.width.max(1),
                height: allocation.height.max(1),
                visual: gtk_widget_get_visual(widget),
                window_type: 6, // GDK_WINDOW_SUBSURFACE
                ..Default::default()
            };
            // GDK treats a subsurface as a toplevel with a transient parent.
            // A logical child parent would apply the allocation offset twice.
            let native = gdk_window_new(ptr::null_mut(), &attributes, 4 | 8 | 32);
            assert!(!native.is_null(), "Cannot create Vulkan subsurface");
            gdk_window_set_transient_for(native, gtk_widget_get_parent_window(widget));
            gtk_widget_unregister_window(widget, window);
            gtk_widget_set_window(widget, native);
            gtk_widget_register_window(widget, native);
            gdk_window_destroy(window);
        } else {
            assert_ne!(
                gdk_window_ensure_native(window),
                0,
                "Cannot create native Vulkan child window"
            );
        }
    }
}

extern "C" fn unmap(_: *mut c_void, pointer: *const Widget) {
    // GDK destroys the wl_surface when hiding a Wayland window. A later map
    // creates a new handle; the existing Vulkan surface cannot be reused.
    let data = unsafe { Widget::retain(pointer) };
    data.lost.set(true);
    data.keys.borrow_mut().clear();
}

extern "C" fn input(widget: *mut c_void, event: *const GdkEvent, pointer: *const Widget) -> i32 {
    let data = unsafe { Widget::retain(pointer) };
    let kind = unsafe { gdk_event_get_event_type(event) };
    let repeat = if kind == 8 || kind == 9 {
        let mut code = 0;
        unsafe { gdk_event_get_keycode(event, &mut code) };
        if kind == 8 {
            !data.keys.borrow_mut().insert(code)
        } else {
            data.keys.borrow_mut().remove(&code);
            false
        }
    } else {
        false
    };
    if kind == 4 {
        unsafe { gtk_widget_grab_focus(widget) };
    }
    if let Some(event) = unsafe { gtk_input_event(event, repeat) } {
        data.sender.send(event);
        return 1;
    }
    0
}

extern "C" fn focus(_: *mut c_void, _: *const GdkEvent, pointer: *const Widget) -> i32 {
    let data = unsafe { Widget::retain(pointer) };
    data.sender.send(WindowEvent::Focus);
    0
}

extern "C" fn blur(_: *mut c_void, _: *const GdkEvent, pointer: *const Widget) -> i32 {
    let data = unsafe { Widget::retain(pointer) };
    data.keys.borrow_mut().clear();
    data.sender.send(WindowEvent::Blur);
    0
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bwindow::ffi::{g_idle_add, gtk_main, gtk_widget_get_scale_factor, gtk_widget_hide};
    use bwindow::{EventLoop, LogicalSize, NativeWindowHandle, WindowBuilder};

    use super::*;

    unsafe extern "C" {
        fn g_main_context_iteration(context: *mut c_void, may_block: i32) -> i32;
        fn gtk_window_resize(window: *mut c_void, width: i32, height: i32);
        fn gtk_widget_queue_draw(widget: *mut c_void);
    }

    fn settle() {
        let until = Instant::now() + Duration::from_millis(200);
        while Instant::now() < until {
            unsafe { g_main_context_iteration(ptr::null_mut(), 0) };
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn submitted_frame(
        surface: &crate::Surface<'_>,
        device: &crate::Device,
        queue: &crate::Queue,
    ) -> crate::SurfaceTexture {
        let frame = match surface.get_current_texture() {
            crate::CurrentSurfaceTexture::Success(frame)
            | crate::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => panic!("expected an available frame after mapping"),
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&crate::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(crate::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: crate::Operations {
                        load: crate::LoadOp::Clear(crate::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: crate::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
            });
        }
        queue.submit([encoder.finish()]);
        frame
    }

    #[test]
    #[ignore = "requires a GTK display and Vulkan; run separately with GDK_BACKEND=x11 and wayland"]
    fn native_surface_uses_client_area_and_survives_resize_and_close() {
        let _event_loop = EventLoop::new();
        let mut window = WindowBuilder::new()
            .title("Vulkan content regression")
            .size(LogicalSize::new(640.0, 480.0))
            .resizable(true)
            .build();
        let attachment = window.attach_content().expect("attach");
        let Some(NativeWindowHandle::Gtk(parent)) = (unsafe { attachment.native_handle() }) else {
            panic!("expected GTK");
        };
        let surface = crate::Instance::default()
            .create_surface(attachment)
            .expect("surface");
        let instance = crate::Instance::default();
        let adapter = instance
            .request_adapter(&crate::RequestAdapterOptions {
                compatible_surface: Some(&surface),
            })
            .expect("adapter");
        let (device, queue) = adapter.request_device(&Default::default()).expect("device");
        let (width, height) = surface.size();
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .expect("config");
        surface.configure(&device, &config);
        if !surface.native.startup_clock.is_null() {
            for _ in 0..8 {
                assert!(matches!(
                    surface.get_current_texture(),
                    crate::CurrentSurfaceTexture::Occluded
                ));
            }
        }
        settle();
        // Exercise actual presentation after the parent has committed, including
        // enough frames for Mesa's FIFO presentation timestamps to take effect.
        for _ in 0..8 {
            let frame = submitted_frame(&surface, &device, &queue);
            queue.present(frame);
            settle();
        }
        unsafe {
            let child = surface.native.widget;
            assert_ne!(gtk_widget_get_window(child), gtk_widget_get_window(parent));
            let parent_handle =
                super::super::surface::platform_window(NativeWindowHandle::Gtk(parent))
                    .expect("parent handle");
            let child_handle =
                super::super::surface::platform_window(NativeWindowHandle::Gtk(child))
                    .expect("child handle");
            match (parent_handle, child_handle) {
                (
                    super::super::surface::PlatformWindow::Xlib { window: parent, .. },
                    super::super::surface::PlatformWindow::Xlib { window: child, .. },
                ) => assert_ne!(parent, child),
                (
                    super::super::surface::PlatformWindow::Wayland {
                        surface: parent, ..
                    },
                    super::super::surface::PlatformWindow::Wayland { surface: child, .. },
                ) => assert_ne!(parent, child),
                _ => panic!("mixed backends"),
            }
            gtk_widget_queue_draw(child);
            assert!(surface.native.redraw.callback.get().is_some());
            let damage = gdk_window_get_update_area(gtk_widget_get_window(child));
            if !damage.is_null() {
                assert_ne!(
                    cairo_region_is_empty(damage),
                    0,
                    "Cairo must not paint the Vulkan window"
                );
                cairo_region_destroy(damage);
            }
            assert_eq!(gtk_widget_get_allocated_width(child), 640);
            assert_eq!(gtk_widget_get_allocated_height(child), 480);
            gtk_window_resize(parent, 800, 600);
            settle();
            assert_eq!(gtk_widget_get_allocated_width(child), 800);
            assert_eq!(gtk_widget_get_allocated_height(child), 600);
            let scale = gtk_widget_get_scale_factor(child.cast()) as u32;
            assert_eq!(surface.size(), (800 * scale, 600 * scale));
            assert!(matches!(
                surface.get_current_texture(),
                crate::CurrentSurfaceTexture::Outdated
            ));
            config.width = 800 * scale;
            config.height = 600 * scale;
            surface.configure(&device, &config);
        }
        let pending_frame = submitted_frame(&surface, &device, &queue);
        extern "C" fn close(data: *mut c_void) -> i32 {
            unsafe { &mut *data.cast::<bwindow::Window>() }.close();
            0
        }
        unsafe {
            g_idle_add(close, (&mut window as *mut bwindow::Window).cast());
            gtk_main();
        }
        assert!(matches!(
            surface.get_current_texture(),
            crate::CurrentSurfaceTexture::Lost
        ));
        assert_eq!(surface.size(), (0, 0));
        queue.present(pending_frame);
        drop(surface);
    }

    #[test]
    #[ignore = "requires a GTK display and Vulkan; run in its own process"]
    fn late_attachment_and_unmap_do_not_reuse_native_surface() {
        let _event_loop = EventLoop::new();
        let mut window = WindowBuilder::new()
            .title("Vulkan late attachment regression")
            .size(LogicalSize::new(640.0, 480.0))
            .build();
        settle();
        let surface = crate::Instance::default()
            .create_surface(window.attach_content().expect("attach"))
            .expect("surface");
        let adapter = crate::Instance::default()
            .request_adapter(&crate::RequestAdapterOptions {
                compatible_surface: Some(&surface),
            })
            .expect("adapter");
        let (device, queue) = adapter.request_device(&Default::default()).expect("device");
        settle();
        let (width, height) = surface.size();
        let config = surface
            .get_default_config(&adapter, width, height)
            .expect("config");
        surface.configure(&device, &config);
        queue.present(submitted_frame(&surface, &device, &queue));
        settle();
        let child = surface.native.widget;
        unsafe { gtk_widget_hide(child.cast()) };
        assert!(matches!(
            surface.get_current_texture(),
            crate::CurrentSurfaceTexture::Lost
        ));
        unsafe { gtk_widget_show(child.cast()) };
        settle();
        assert!(matches!(
            surface.get_current_texture(),
            crate::CurrentSurfaceTexture::Lost
        ));
        assert!(surface.native.is_lost());
        // Dropping the renderer must cancel any outstanding callback before its
        // widget and callback data are freed, even while the parent stays open.
        surface.native.request_animation_frame();
        drop(surface);
        settle();
        window.close();
    }
}
