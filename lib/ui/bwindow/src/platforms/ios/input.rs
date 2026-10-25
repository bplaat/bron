/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use objc2::msg_send;
use objc2::runtime::AnyObject as Object;

use super::headers::*;
use crate::{LogicalPoint, Touch};

/// Translate all changed UIKit touches that started on this view.
///
/// # Safety
/// `view` must be a live UIView and `touches` a live NSSet of UITouch on the main thread.
pub(super) unsafe fn ios_touches(view: *mut Object, touches: *mut Object) -> Vec<Touch> {
    let array: *mut Object = unsafe { msg_send![touches, allObjects] };
    let count: usize = unsafe { msg_send![array, count] };
    (0..count)
        .filter_map(|index| {
            let touch: *mut Object = unsafe { msg_send![array, objectAtIndex:index] };
            let target: *mut Object = unsafe { msg_send![touch, view] };
            if target != view {
                return None;
            }
            let point: CGPoint = unsafe { msg_send![touch, locationInView:view] };
            Some(Touch {
                identifier: touch as u64,
                position: LogicalPoint::new(point.x as f32, point.y as f32),
            })
        })
        .collect()
}
