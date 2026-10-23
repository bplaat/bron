/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use objc2::msg_send;
use objc2::runtime::AnyObject as Object;

use super::headers::*;
use crate::{ButtonState, LogicalPoint, Modifiers, MouseButton, MouseEvent, WindowEvent};

/// Translate a UIKit touch set into a left mouse button pointer event.
///
/// # Safety
/// `view` must be a live UIView and `touches` a live NSSet of UITouch on the main thread.
pub(super) unsafe fn ios_touch_event(
    view: *mut Object,
    touches: *mut Object,
    state: Option<ButtonState>,
) -> WindowEvent {
    let touch: *mut Object = unsafe { msg_send![touches, anyObject] };
    let point: CGPoint = unsafe { msg_send![touch, locationInView:view] };
    let position = LogicalPoint::new(point.x as f32, point.y as f32);
    let modifiers = Modifiers::NONE;
    match state {
        Some(state) => {
            let event = MouseEvent {
                button: MouseButton::Left,
                position,
                modifiers,
            };
            match state {
                ButtonState::Pressed => WindowEvent::MouseDown(event),
                ButtonState::Released => WindowEvent::MouseUp(event),
            }
        }
        None => WindowEvent::MouseMove {
            position,
            movement: LogicalPoint::default(),
            modifiers,
        },
    }
}
