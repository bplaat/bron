/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::*;

#[test]
fn queued_redraw_uses_the_renderer_attached_before_delivery() {
    let host = ContentHost::new(WindowId::new(), None, false);
    host.request_redraw();
    let attachment = host.attach().expect("attachment");
    let scheduled = Rc::new(Cell::new(0));
    let output = scheduled.clone();
    attachment.on_redraw(move || output.set(output.get() + 1));
    let painted = Rc::new(Cell::new(false));
    let output = painted.clone();
    crate::dispatch::start(move |event| {
        assert!(matches!(
            event,
            crate::Event::Window(_, WindowEvent::RedrawRequested)
        ));
        output.set(true);
    });
    assert_eq!(scheduled.get(), 1);
    assert!(!painted.get(), "queued redraw bypassed native paint");
    assert!(attachment.event_sender().redraw(|| {}));
    assert!(painted.get(), "native paint was not delivered");
    assert_eq!(
        scheduled.get(),
        1,
        "native paint must not reschedule itself"
    );
}

#[test]
fn queued_redraw_is_discarded_after_close() {
    let host = ContentHost::new(WindowId::new(), None, false);
    host.request_redraw();
    host.close();
    crate::dispatch::start(|_| panic!("redraw delivered after close"));
}

#[test]
fn locked_motion_keeps_the_position_even_when_relative_motion_is_zero() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let at = LogicalPoint::new(10.0, 20.0);
    let moved = |position, movement| WindowEvent::MouseMove {
        position,
        movement,
        modifiers: crate::Modifiers::NONE,
    };
    host.track_pointer(moved(at, LogicalPoint::default()));
    host.pointer_locked.set(true);
    for movement in [LogicalPoint::new(3.0, -4.0), LogicalPoint::default()] {
        let WindowEvent::MouseMove {
            position,
            movement: actual,
            ..
        } = host.track_pointer(moved(LogicalPoint::default(), movement))
        else {
            panic!("expected mouse move");
        };
        assert_eq!(position, at);
        assert_eq!(actual, movement);
    }
    host.pointer_locked.set(false);
}

#[test]
fn queued_touch_start_can_cancel_compatibility_mouse_events() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let sender = host.event_sender();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    // Queue before installing the handler so cancellation must be checked at delivery.
    sender.send(WindowEvent::TouchStart(crate::TouchEvent {
        changed_touches: vec![crate::Touch {
            identifier: 1,
            position: LogicalPoint::default(),
        }],
        ..crate::TouchEvent::default()
    }));
    crate::dispatch::start(move |event| {
        if let crate::Event::Window(_, WindowEvent::TouchStart(touch)) = event {
            touch.prevent_default();
            output.set(output.get() + 1);
        } else {
            panic!("unexpected compatibility mouse event");
        }
    });
    assert_eq!(calls.get(), 1);
}

#[test]
fn readiness_runs_once_and_late_registration_runs_immediately() {
    let host = ContentHost::new(WindowId::new(), None, false);
    host.ready.set(false);
    let attachment = host.attach().expect("attachment");
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    attachment.on_ready(move || output.set(output.get() + 1));
    assert_eq!(calls.get(), 0);
    host.ready();
    host.ready();
    assert_eq!(calls.get(), 1);
    assert!(attachment.is_ready());
    let output = calls.clone();
    attachment.on_ready(move || output.set(output.get() + 1));
    assert_eq!(calls.get(), 2);
}

#[test]
fn pending_readiness_is_canceled_on_disconnect_or_close() {
    for close in [false, true] {
        let host = ContentHost::new(WindowId::new(), None, false);
        host.ready.set(false);
        let attachment = host.attach().expect("attachment");
        attachment.on_ready(|| panic!("canceled readiness callback"));
        if close {
            host.close();
            assert!(!attachment.is_ready());
        } else {
            drop(attachment);
        }
        host.ready();
    }
}

#[test]
fn replacing_hooks_allows_captured_destructors_to_register_again() {
    struct OnDrop(Box<dyn Fn()>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            (self.0)();
        }
    }

    for kind in 0..4 {
        let host = ContentHost::new(WindowId::new(), None, false);
        let attachment = Rc::new(host.attach().expect("attachment"));
        let weak = Rc::downgrade(&attachment);
        let dropped = Rc::new(Cell::new(false));
        let output = dropped.clone();
        let guard = OnDrop(Box::new(move || {
            let attachment = weak.upgrade().expect("live attachment");
            match kind {
                0 => attachment.on_resize(|_, _| {}),
                1 => attachment.on_close(|| {}),
                2 => attachment.on_redraw(|| {}),
                _ => attachment.on_scale_factor_changed(|_| {}),
            }
            output.set(true);
        }));
        match kind {
            0 => {
                attachment.on_resize(move |_, _| {
                    let _keep_alive = &guard;
                });
                attachment.on_resize(|_, _| {});
            }
            1 => {
                attachment.on_close(move || {
                    let _keep_alive = &guard;
                });
                attachment.on_close(|| {});
            }
            2 => {
                attachment.on_redraw(move || {
                    let _keep_alive = &guard;
                });
                attachment.on_redraw(|| {});
            }
            _ => {
                attachment.on_scale_factor_changed(move |_| {
                    let _keep_alive = &guard;
                });
                attachment.on_scale_factor_changed(|_| {});
            }
        }
        assert!(dropped.get());
    }
}

#[test]
fn repaint_hooks_are_disconnected_on_close_and_drop() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let attachment = host.attach().expect("attachment");
    let count = Rc::new(Cell::new(0));
    let output = count.clone();
    attachment.on_redraw(move || output.set(output.get() + 1));
    host.request_redraw();
    assert_eq!(count.get(), 1);
    host.close();
    host.request_redraw();
    assert_eq!(count.get(), 1);

    let host = ContentHost::new(WindowId::new(), None, false);
    let attachment = host.attach().expect("attachment");
    attachment.on_redraw(|| panic!("disconnected"));
    drop(attachment);
    host.request_redraw();
}

#[test]
fn animation_frames_coalesce_into_one_redraw_per_display_frame() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let attachment = host.attach().expect("attachment");
    let count = Rc::new(Cell::new(0));
    let output = count.clone();
    attachment.on_redraw(move || output.set(output.get() + 1));

    let pending = &attachment.layer.animation_frame_pending;

    // Without a native window there is no frame source, so requests are dropped.
    host.request_animation_frame();
    assert!(!pending.get());

    // Display frames redraw once per pending request.
    pending.set(true);
    host.animation_frame();
    host.animation_frame();
    assert_eq!(count.get(), 1);

    // A redraw delivered before the display frame satisfies the request.
    pending.set(true);
    attachment.event_sender().redraw(|| {});
    host.animation_frame();
    assert_eq!(count.get(), 1);

    pending.set(true);
    host.close();
    host.animation_frame();
    assert_eq!(count.get(), 1);
}

#[test]
fn scale_changes_notify_once_per_value() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let attachment = host.attach().expect("attachment");
    let count = Rc::new(Cell::new(0));
    let output = count.clone();
    attachment.on_scale_factor_changed(move |_| output.set(output.get() + 1));
    assert!(host.scale_factor_changed(2.0));
    assert!(!host.scale_factor_changed(2.0));
    assert_eq!(attachment.scale_factor(), 2.0);
    assert_eq!(count.get(), 1);
}

#[test]
fn content_host_is_created_once_on_first_use() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let count = Rc::new(Cell::new(0));
    let output = count.clone();
    host.set_content_factory(move || {
        output.set(output.get() + 1);
        Some(NativeContentHandle::Gtk(std::ptr::null_mut()))
    });
    assert!(host.content_handle().is_some());
    assert!(host.content_handle().is_some());
    assert_eq!(count.get(), 1);
    host.close();
    assert!(host.content_handle().is_none());
}

#[test]
fn layers_are_attached_once() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let layer = WindowLayer::new(host.clone(), host.add_layer());
    let attachment = layer.attach().expect("first attachment");
    assert!(matches!(layer.attach(), Err(AttachError::Occupied)));
    drop(attachment);
    assert!(matches!(layer.attach(), Err(AttachError::Occupied)));
}

#[test]
fn content_is_attached_once_and_excludes_layers() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let layer = WindowLayer::new(host.clone(), host.add_layer());
    let content = host.attach_content().expect("content");
    assert!(matches!(host.attach_content(), Err(AttachError::Occupied)));
    assert_eq!(content.stack_position(), 0);
    // One item takes the whole window, or layers stack in it.
    assert!(matches!(layer.attach(), Err(AttachError::Unsupported)));
    drop(content);
    let attachment = layer.attach().expect("layer without content");
    assert!(matches!(
        host.attach_content(),
        Err(AttachError::Unsupported)
    ));
    drop(attachment);
}

fn layers(host: &Rc<ContentHost>, count: usize) -> Vec<WindowAttachment> {
    (0..count)
        .map(|_| {
            let layer = WindowLayer::new(host.clone(), host.add_layer());
            layer.attach().expect("attachment")
        })
        .collect()
}

#[test]
fn window_redraws_and_frames_reach_every_layer_and_layer_ones_only_it() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let layers = layers(&host, 2);
    let counts: Vec<_> = layers
        .iter()
        .map(|layer| {
            let count = Rc::new(Cell::new(0));
            let output = count.clone();
            layer.on_redraw(move || output.set(output.get() + 1));
            count
        })
        .collect();
    host.request_redraw();
    assert_eq!([counts[0].get(), counts[1].get()], [1, 1]);

    // A display frame redraws the layers with a pending frame.
    layers[1].layer.animation_frame_pending.set(true);
    host.animation_frame();
    assert_eq!([counts[0].get(), counts[1].get()], [1, 2]);

    // The paint of one layer only satisfies its own frame.
    for layer in &layers {
        layer.layer.animation_frame_pending.set(true);
    }
    layers[0].event_sender().redraw(|| {});
    host.animation_frame();
    assert_eq!([counts[0].get(), counts[1].get()], [1, 3]);
}

#[test]
fn framed_layers_resize_with_their_frame_and_the_scale_not_the_window() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let attachments = layers(&host, 2);
    let sizes: Vec<_> = attachments
        .iter()
        .map(|attachment| {
            let size = Rc::new(Cell::new((0, 0)));
            let output = size.clone();
            attachment.on_resize(move |width, height| output.set((width, height)));
            size
        })
        .collect();
    let framed = WindowLayer::new(host.clone(), attachments[1].layer.clone());
    framed.set_frame(Some(LogicalRect::new(0.0, -50.0, 10.0, 20.0)));
    assert_eq!(sizes[1].get(), (10, 20));
    host.resize(100, 80);
    assert_eq!(sizes[0].get(), (100, 80));
    assert_eq!(sizes[1].get(), (10, 20));
    host.scale_factor_changed(2.0);
    assert_eq!(sizes[1].get(), (20, 40));
    // Moving keeps the size, so the layer is not resized.
    sizes[1].set((0, 0));
    framed.set_frame(Some(LogicalRect::new(0.0, -90.0, 10.0, 20.0)));
    assert_eq!(sizes[1].get(), (0, 0));
    framed.set_frame(None);
    assert_eq!(sizes[1].get(), (100, 80));
}

#[test]
fn dropped_layers_leave_the_stack() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let mut attachments = layers(&host, 3);
    attachments.remove(1);
    assert_eq!(host.layers().len(), 2);
    drop(attachments);
    assert!(host.layers().is_empty());
}

#[test]
fn closing_invalidates_handles_and_runs_teardown_once() {
    let host = ContentHost::new(WindowId::new(), None, false);
    let attachment = host.attach().expect("first attachment");
    let count = Rc::new(Cell::new(0));
    let output = count.clone();
    attachment.on_close(move || output.set(output.get() + 1));
    host.close();
    host.close();
    assert!(attachment.is_closed());
    assert_eq!(count.get(), 1);
    assert!(matches!(host.attach(), Err(AttachError::Closed)));
}
