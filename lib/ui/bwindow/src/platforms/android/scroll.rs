/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native scrolling of the window content: a one-finger drag past the touch slop scrolls the
//! content instead of reaching the app, which gets the touch canceled like by a native scroll
//! view, and lifting the finger flings on with the `android.widget.OverScroller` physics at the
//! display frames. A mouse wheel scrolls too. The window surface belongs to the native code, so
//! there are no scroll bar views to show.

use std::cell::{Cell, RefCell};
use std::time::Instant;

use jni::objects::{Global, JObject};
use jni::{JValue, jni_sig, jni_str};

use super::activity::{android_activity, android_with_env};
use super::event_loop::{android_density, android_post_frame, android_window_size};
use super::window;
use crate::{LogicalPoint, LogicalSize, TouchEvent, WindowEvent};

// The distance a finger moves before it scrolls, and of a wheel notch, in logical pixels.
const TOUCH_SLOP: f32 = 8.0;
const LINE: f32 = 40.0;

struct Scroll {
    size: LogicalSize,
    offset: Cell<(f32, f32)>,
    scroller: Global<JObject<'static>>,
    // The finger that may drag: its identifier, last position and whether it scrolls.
    finger: Cell<Option<(u64, LogicalPoint, bool)>>,
    // Recent finger positions with their times, for the fling speed.
    samples: RefCell<Vec<(Instant, LogicalPoint)>>,
    flinging: Cell<bool>,
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static SCROLL: RefCell<Option<Scroll>> = const { RefCell::new(None) };
}

/// Scroll the content natively for content of a size, or stop with none.
pub(super) fn set_size(size: Option<LogicalSize>) {
    let Some(size) = size else {
        SCROLL.with_borrow_mut(|scroll| scroll.take());
        return;
    };
    SCROLL.with_borrow_mut(|scroll| match scroll {
        Some(scroll) => scroll.size = size,
        None => {
            let scroller = android_with_env(|env| {
                let class = env.find_class(jni_str!("android/widget/OverScroller"))?;
                let scroller = env.new_object(
                    &class,
                    jni_sig!("(Landroid/content/Context;)V"),
                    &[android_activity().into()],
                )?;
                env.new_global_ref(&scroller)
            });
            *scroll = Some(Scroll {
                size,
                offset: Cell::new((0.0, 0.0)),
                scroller,
                finger: Cell::new(None),
                samples: RefCell::new(Vec::new()),
                flinging: Cell::new(false),
            });
        }
    });
    // A smaller content moves the offset inside it.
    let offset = offset();
    scroll_to(offset);
}

pub(super) fn offset() -> LogicalPoint {
    SCROLL.with_borrow(|scroll| {
        scroll
            .as_ref()
            .map_or(LogicalPoint::new(0.0, 0.0), |scroll| {
                let (x, y) = scroll.offset.get();
                LogicalPoint::new(x, y)
            })
    })
}

/// Scroll to an offset within the content.
pub(super) fn scroll_to(offset: LogicalPoint) {
    let changed = SCROLL.with_borrow(|scroll| {
        let scroll = scroll.as_ref()?;
        let (max_x, max_y) = scroll.max();
        let offset = (offset.x.clamp(0.0, max_x), offset.y.clamp(0.0, max_y));
        (scroll.offset.replace(offset) != offset).then(|| LogicalPoint::new(offset.0, offset.1))
    });
    if let Some(offset) = changed {
        window::send(WindowEvent::Scroll(offset));
    }
}

impl Scroll {
    // The largest offsets, where the content ends at the window edges.
    fn max(&self) -> (f32, f32) {
        let density = android_density();
        let (width, height) = android_window_size();
        (
            (self.size.width - width as f32 / density).max(0.0),
            (self.size.height - height as f32 / density).max(0.0),
        )
    }

    fn stop_fling(&self) {
        if self.flinging.replace(false) {
            android_with_env(|env| {
                env.call_method(
                    self.scroller.as_obj(),
                    jni_str!("forceFinished"),
                    jni_sig!("(Z)V"),
                    &[JValue::Bool(true)],
                )?
                .v()
            });
        }
    }
}

/// Scroll with a mouse wheel, true while scrolling natively.
pub(super) fn wheel(dx: f32, dy: f32) -> bool {
    let Some(offset) = SCROLL.with_borrow(|scroll| {
        scroll.as_ref().map(|scroll| {
            scroll.stop_fling();
            scroll.offset.get()
        })
    }) else {
        return false;
    };
    scroll_to(LogicalPoint::new(
        offset.0 + dx * LINE,
        offset.1 + dy * LINE,
    ));
    true
}

/// Scroll with a one-finger drag, true when the touch event scrolled instead of reaching the
/// app.
pub(super) fn touch(event: &WindowEvent) -> bool {
    let (WindowEvent::TouchStart(touch)
    | WindowEvent::TouchMove(touch)
    | WindowEvent::TouchEnd(touch)
    | WindowEvent::TouchCancel(touch)) = event
    else {
        return false;
    };
    let action = SCROLL.with_borrow(|scroll| {
        let scroll = scroll.as_ref()?;
        Some(match event {
            WindowEvent::TouchStart(_) => {
                scroll.stop_fling();
                // Only one finger scrolls, a second one goes to the app.
                let single = touch.changed_touches.len() == 1 && scroll.finger.get().is_none();
                if single {
                    let finger = touch.changed_touches[0];
                    scroll
                        .finger
                        .set(Some((finger.identifier, finger.position, false)));
                    *scroll.samples.borrow_mut() = vec![(Instant::now(), finger.position)];
                } else {
                    scroll.finger.set(None);
                }
                Action::Pass
            }
            WindowEvent::TouchMove(_) => {
                let Some((id, last, dragging)) = scroll.finger.get() else {
                    return Some(Action::Pass);
                };
                let Some(finger) = touch.changed_touches.iter().find(|t| t.identifier == id) else {
                    return Some(Action::Pass);
                };
                let (dx, dy) = (finger.position.x - last.x, finger.position.y - last.y);
                if !dragging && dx.hypot(dy) < TOUCH_SLOP {
                    return Some(Action::Pass);
                }
                scroll.finger.set(Some((id, finger.position, true)));
                let mut samples = scroll.samples.borrow_mut();
                samples.push((Instant::now(), finger.position));
                samples.retain(|(time, _)| time.elapsed().as_millis() < 100);
                let (x, y) = scroll.offset.get();
                let offset = LogicalPoint::new(x - dx, y - dy);
                if dragging {
                    Action::Scroll(offset)
                } else {
                    Action::Start(offset, *finger)
                }
            }
            WindowEvent::TouchEnd(_) | WindowEvent::TouchCancel(_) => {
                let Some((id, _, dragging)) = scroll.finger.get() else {
                    return Some(Action::Pass);
                };
                if !touch.changed_touches.iter().any(|t| t.identifier == id) {
                    return Some(Action::Pass);
                }
                scroll.finger.set(None);
                if !dragging {
                    return Some(Action::Pass);
                }
                Action::Fling(velocity(&scroll.samples.borrow()))
            }
            _ => Action::Pass,
        })
    });
    match action {
        None | Some(Action::Pass) => false,
        Some(Action::Start(offset, finger)) => {
            // The app gets the touch canceled, then the content scrolls.
            window::send(WindowEvent::TouchCancel(TouchEvent {
                changed_touches: vec![finger],
                ..TouchEvent::default()
            }));
            scroll_to(offset);
            true
        }
        Some(Action::Scroll(offset)) => {
            scroll_to(offset);
            true
        }
        Some(Action::Fling(velocity)) => {
            fling(velocity);
            true
        }
    }
}

enum Action {
    Pass,
    Start(LogicalPoint, crate::Touch),
    Scroll(LogicalPoint),
    Fling((f32, f32)),
}

// The finger speed in logical pixels per second over the recent positions.
fn velocity(samples: &[(Instant, LogicalPoint)]) -> (f32, f32) {
    let (Some((start, from)), Some((end, to))) = (samples.first(), samples.last()) else {
        return (0.0, 0.0);
    };
    let seconds = end.duration_since(*start).as_secs_f32();
    if seconds <= 0.0 {
        return (0.0, 0.0);
    }
    ((to.x - from.x) / seconds, (to.y - from.y) / seconds)
}

// Fling on with the OverScroller physics in physical pixels, at the display frames.
fn fling((vx, vy): (f32, f32)) {
    let density = android_density();
    let started = SCROLL.with_borrow(|scroll| {
        let scroll = scroll.as_ref()?;
        let (x, y) = scroll.offset.get();
        let (max_x, max_y) = scroll.max();
        let pixels = |value: f32| (value * density).round() as i32;
        android_with_env(|env| {
            env.call_method(
                scroll.scroller.as_obj(),
                jni_str!("fling"),
                jni_sig!("(IIIIIIII)V"),
                &[
                    pixels(x),
                    pixels(y),
                    pixels(-vx),
                    pixels(-vy),
                    0,
                    pixels(max_x),
                    0,
                    pixels(max_y),
                ]
                .map(JValue::Int),
            )?
            .v()
        });
        scroll.flinging.set(true);
        Some(())
    });
    if started.is_some() {
        android_post_frame(fling_frame);
    }
}

fn fling_frame() {
    let density = android_density();
    let offset = SCROLL.with_borrow(|scroll| {
        let scroll = scroll.as_ref().filter(|scroll| scroll.flinging.get())?;
        android_with_env(|env| {
            let scroller = scroll.scroller.as_obj();
            let running = env
                .call_method(
                    scroller,
                    jni_str!("computeScrollOffset"),
                    jni_sig!("()Z"),
                    &[],
                )?
                .z()?;
            let x = env
                .call_method(scroller, jni_str!("getCurrX"), jni_sig!("()I"), &[])?
                .i()?;
            let y = env
                .call_method(scroller, jni_str!("getCurrY"), jni_sig!("()I"), &[])?
                .i()?;
            Ok(Some((running, x, y)))
        })
    });
    let Some((running, x, y)) = offset else {
        return;
    };
    scroll_to(LogicalPoint::new(x as f32 / density, y as f32 / density));
    if running {
        android_post_frame(fling_frame);
    } else {
        SCROLL.with_borrow(|scroll| {
            if let Some(scroll) = scroll {
                scroll.flinging.set(false);
            }
        });
    }
}
