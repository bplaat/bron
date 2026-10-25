/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::{LogicalPoint, Modifiers, MouseButton, MouseEvent, WindowEvent};

// Only the first finger emulates a mouse. Canceling touchstart suppresses that
// compatibility gesture until every finger has lifted.
#[derive(Default)]
pub(crate) struct TouchMouse {
    active: Vec<u64>,
    primary: Option<u64>,
    suppressed: bool,
}

impl TouchMouse {
    pub(crate) fn events(&mut self, event: &WindowEvent) -> Vec<WindowEvent> {
        let (touch, start, end) = match event {
            WindowEvent::TouchStart(touch) => (touch, true, false),
            WindowEvent::TouchMove(touch) => (touch, false, false),
            WindowEvent::TouchEnd(touch) | WindowEvent::TouchCancel(touch) => (touch, false, true),
            _ => return Vec::new(),
        };
        if start {
            if self.active.is_empty() {
                self.primary = touch.changed_touches.first().map(|touch| touch.identifier);
                self.suppressed = touch.default_prevented();
            }
            for changed in &touch.changed_touches {
                if !self.active.contains(&changed.identifier) {
                    self.active.push(changed.identifier);
                }
            }
        }
        let primary = touch
            .changed_touches
            .iter()
            .find(|touch| Some(touch.identifier) == self.primary);
        let mut events = Vec::new();
        if let Some(primary) = primary {
            if !self.suppressed {
                let mouse = MouseEvent {
                    button: MouseButton::Left,
                    position: primary.position,
                    modifiers: Modifiers::NONE,
                };
                if !end {
                    events.push(WindowEvent::MouseMove {
                        position: primary.position,
                        movement: LogicalPoint::default(),
                        modifiers: Modifiers::NONE,
                    });
                }
                if start {
                    events.push(WindowEvent::MouseDown(mouse));
                } else if end {
                    events.extend([WindowEvent::MouseUp(mouse), WindowEvent::MouseLeave]);
                }
            }
            if end {
                self.primary = None;
            }
        }
        if end {
            self.active.retain(|identifier| {
                !touch
                    .changed_touches
                    .iter()
                    .any(|touch| touch.identifier == *identifier)
            });
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Touch, TouchEvent};

    fn touches(identifiers: &[u64]) -> TouchEvent {
        TouchEvent {
            changed_touches: identifiers
                .iter()
                .map(|&identifier| Touch {
                    identifier,
                    position: LogicalPoint::new(identifier as f32, 20.0),
                })
                .collect(),
            ..TouchEvent::default()
        }
    }

    #[test]
    fn canceling_start_suppresses_the_whole_mouse_gesture() {
        let mut mouse = TouchMouse::default();
        let start = touches(&[1]);
        start.clone().prevent_default();
        assert!(start.default_prevented());
        assert!(mouse.events(&WindowEvent::TouchStart(start)).is_empty());
        assert!(
            mouse
                .events(&WindowEvent::TouchMove(touches(&[1])))
                .is_empty()
        );
        assert!(
            mouse
                .events(&WindowEvent::TouchEnd(touches(&[1])))
                .is_empty()
        );
        assert!(matches!(
            mouse
                .events(&WindowEvent::TouchStart(touches(&[2])))
                .as_slice(),
            [WindowEvent::MouseMove { .. }, WindowEvent::MouseDown(_)]
        ));
    }

    #[test]
    fn secondary_fingers_never_take_over_the_mouse() {
        let mut mouse = TouchMouse::default();
        assert_eq!(
            mouse.events(&WindowEvent::TouchStart(touches(&[1]))).len(),
            2
        );
        assert!(
            mouse
                .events(&WindowEvent::TouchStart(touches(&[2])))
                .is_empty()
        );
        assert!(
            mouse
                .events(&WindowEvent::TouchMove(touches(&[2])))
                .is_empty()
        );
        assert!(matches!(
            mouse
                .events(&WindowEvent::TouchCancel(touches(&[1])))
                .as_slice(),
            [WindowEvent::MouseUp(_), WindowEvent::MouseLeave]
        ));
        assert!(
            mouse
                .events(&WindowEvent::TouchStart(touches(&[3])))
                .is_empty()
        );
        assert!(
            mouse
                .events(&WindowEvent::TouchEnd(touches(&[2, 3])))
                .is_empty()
        );
        assert_eq!(
            mouse.events(&WindowEvent::TouchStart(touches(&[4]))).len(),
            2
        );
    }
}
