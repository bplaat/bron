/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! On-screen touch controls like the Welpen games: a floating joystick on the left side,
//! dragging on the right side looks around, and round buttons for the other actions.
//! Like Minecraft Pocket Edition, tapping the world places a block and holding breaks it.

use std::time::{Duration, Instant};

use bcanvas::{Color, TextAlign, TextBaseline};
use bwindow::{LogicalInsets, LogicalPoint, LogicalSize, Touch};

use crate::hud::{Hud, MARGIN, Sprite};

const JOYSTICK_SPLIT: f32 = 0.45;
const JOYSTICK_RADIUS: f32 = 48.0;
const JOYSTICK_MAX: f32 = 44.0;
const THUMB_RADIUS: f32 = 20.0;
const BUTTON_RADIUS: f32 = 34.0;
const SMALL_BUTTON_RADIUS: f32 = 22.0;
const BUTTON_SPACING: f32 = 84.0;
const BUTTON_MARGIN: f32 = 28.0;
// A touch that moves less than this is a tap or a hold, not a look around drag.
const TAP_DISTANCE: f32 = 10.0;
const LONG_PRESS: Duration = Duration::from_millis(400);
const BREAK_REPEAT: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    Menu,
    Jump,
    Down,
    Fly,
    Night,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    Press(Button),
    Release(Button),
    Look(f32, f32),
    Place,
    Break,
}

// The positions of the controls in logical pixels for the window size and safe area.
pub(crate) struct Layout {
    buttons: Vec<(Button, LogicalPoint, f32)>,
    joystick_rest: LogicalPoint,
    split: f32,
    hotbar_band: (f32, f32),
}

impl Layout {
    pub(crate) fn new(size: LogicalSize, safe_area: LogicalInsets, flying: bool) -> Self {
        let right = size.width - safe_area.right - BUTTON_MARGIN - BUTTON_RADIUS;
        let bottom = size.height - safe_area.bottom - BUTTON_MARGIN - BUTTON_RADIUS;
        let small = |index: f32| {
            LogicalPoint::new(
                safe_area.left
                    + MARGIN
                    + SMALL_BUTTON_RADIUS
                    + index * (SMALL_BUTTON_RADIUS * 2.0 + MARGIN),
                safe_area.top + MARGIN + SMALL_BUTTON_RADIUS,
            )
        };
        // While flying Up and Down are stacked in the corner, walking only has Jump.
        let jump_y = if flying {
            bottom - BUTTON_SPACING
        } else {
            bottom
        };
        let mut buttons = vec![
            (
                Button::Jump,
                LogicalPoint::new(right, jump_y),
                BUTTON_RADIUS,
            ),
            (Button::Menu, small(0.0), SMALL_BUTTON_RADIUS),
            (Button::Fly, small(1.0), SMALL_BUTTON_RADIUS),
            (Button::Night, small(2.0), SMALL_BUTTON_RADIUS),
        ];
        if flying {
            buttons.push((
                Button::Down,
                LogicalPoint::new(right, bottom),
                BUTTON_RADIUS,
            ));
        }
        Self {
            buttons,
            joystick_rest: LogicalPoint::new(
                safe_area.left + BUTTON_MARGIN + JOYSTICK_RADIUS,
                size.height - safe_area.bottom - BUTTON_MARGIN - JOYSTICK_RADIUS,
            ),
            split: size.width * JOYSTICK_SPLIT,
            // Between the joystick and the column of Jump, Up and Down buttons.
            hotbar_band: (
                safe_area.left + BUTTON_MARGIN + JOYSTICK_RADIUS * 2.0 + MARGIN,
                right - BUTTON_RADIUS - MARGIN,
            ),
        }
    }

    // The free horizontal range at the bottom of the screen for the hotbar.
    pub(crate) const fn hotbar_band(&self) -> (f32, f32) {
        self.hotbar_band
    }

    fn button_at(&self, position: LogicalPoint) -> Option<Button> {
        self.buttons
            .iter()
            .find(|(_, center, radius)| {
                (position.x - center.x).hypot(position.y - center.y) <= *radius
            })
            .map(|&(button, ..)| button)
    }
}

struct Joystick {
    identifier: u64,
    origin: LogicalPoint,
    position: LogicalPoint,
}

// The touch on the look side, which also places on a tap and breaks while held.
struct Look {
    identifier: u64,
    start: LogicalPoint,
    last: LogicalPoint,
    started: Instant,
    dragged: bool,
    next_break: Instant,
    broke: bool,
}

struct Sprites {
    scale: f32,
    flying: bool,
    base: Sprite,
    thumb: Sprite,
    buttons: Vec<(Button, Sprite, Sprite)>,
}

#[derive(Default)]
pub(crate) struct TouchControls {
    joystick: Option<Joystick>,
    look: Option<Look>,
    held: Vec<(u64, Button)>,
    sprites: Option<Sprites>,
}

impl TouchControls {
    pub(crate) fn start(
        &mut self,
        touches: &[Touch],
        layout: &Layout,
        now: Instant,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        for touch in touches {
            if let Some(button) = layout.button_at(touch.position) {
                self.held.push((touch.identifier, button));
                actions.push(Action::Press(button));
            } else if touch.position.x < layout.split {
                if self.joystick.is_none() {
                    self.joystick = Some(Joystick {
                        identifier: touch.identifier,
                        origin: touch.position,
                        position: touch.position,
                    });
                }
            } else if self.look.is_none() {
                self.look = Some(Look {
                    identifier: touch.identifier,
                    start: touch.position,
                    last: touch.position,
                    started: now,
                    dragged: false,
                    next_break: now + LONG_PRESS,
                    broke: false,
                });
            }
        }
        actions
    }

    pub(crate) fn moved(&mut self, touches: &[Touch]) -> Vec<Action> {
        let mut actions = Vec::new();
        for touch in touches {
            if let Some(joystick) = &mut self.joystick
                && joystick.identifier == touch.identifier
            {
                joystick.position = touch.position;
            }
            if let Some(look) = &mut self.look
                && look.identifier == touch.identifier
            {
                actions.push(Action::Look(
                    touch.position.x - look.last.x,
                    touch.position.y - look.last.y,
                ));
                look.last = touch.position;
                look.dragged |= (touch.position.x - look.start.x)
                    .hypot(touch.position.y - look.start.y)
                    >= TAP_DISTANCE;
            }
        }
        actions
    }

    pub(crate) fn end(&mut self, touches: &[Touch], now: Instant) -> Vec<Action> {
        self.finish(touches, Some(now))
    }

    pub(crate) fn cancel(&mut self, touches: &[Touch]) -> Vec<Action> {
        self.finish(touches, None)
    }

    fn finish(&mut self, touches: &[Touch], now: Option<Instant>) -> Vec<Action> {
        let mut actions = Vec::new();
        for touch in touches {
            if self
                .joystick
                .as_ref()
                .is_some_and(|joystick| joystick.identifier == touch.identifier)
            {
                self.joystick = None;
            }
            if let Some(look) = self
                .look
                .take_if(|look| look.identifier == touch.identifier)
                && !look.dragged
                && !look.broke
                && now.is_some_and(|now| now - look.started < LONG_PRESS)
            {
                actions.push(Action::Place);
            }
            self.held.retain(|&(identifier, button)| {
                let ended = identifier == touch.identifier;
                if ended {
                    actions.push(Action::Release(button));
                }
                !ended
            });
        }
        actions
    }

    // Breaks blocks while the look touch is held still, called every frame.
    pub(crate) fn update(&mut self, now: Instant) -> Vec<Action> {
        match &mut self.look {
            Some(look) if !look.dragged && now >= look.next_break => {
                look.broke = true;
                look.next_break = now + BREAK_REPEAT;
                vec![Action::Break]
            }
            _ => Vec::new(),
        }
    }

    // Releases every touch, for when the window loses focus.
    pub(crate) fn reset(&mut self) -> Vec<Action> {
        self.joystick = None;
        self.look = None;
        self.held
            .drain(..)
            .map(|(_, button)| Action::Release(button))
            .collect()
    }

    // The thumb offset from the joystick center, at most JOYSTICK_MAX long.
    fn thumb(&self) -> (f32, f32) {
        self.joystick.as_ref().map_or((0.0, 0.0), |joystick| {
            let (dx, dy) = (
                joystick.position.x - joystick.origin.x,
                joystick.position.y - joystick.origin.y,
            );
            let length = dx.hypot(dy);
            let clamped = length.min(JOYSTICK_MAX);
            if length > 0.0 {
                (dx / length * clamped, dy / length * clamped)
            } else {
                (0.0, 0.0)
            }
        })
    }

    // The joystick direction with x to the right and y forward, at most 1 long.
    pub(crate) fn stick(&self) -> [f32; 2] {
        let (dx, dy) = self.thumb();
        [dx / JOYSTICK_MAX, -dy / JOYSTICK_MAX]
    }

    // Draws the button sprites again when the scale or the flight buttons changed.
    pub(crate) fn prepare(
        &mut self,
        hud: &Hud,
        device: &bwebgpu::Device,
        queue: &bwebgpu::Queue,
        scale: f32,
        flying: bool,
    ) {
        if self
            .sprites
            .as_ref()
            .is_some_and(|sprites| sprites.scale == scale && sprites.flying == flying)
        {
            return;
        }
        let circle = |radius: f32, pressed: bool, label: &str| {
            let size = (radius * 2.0).ceil() + 2.0;
            let mut sprite = hud.sprite(device, size as u32, size as u32, scale);
            sprite.redraw(queue, |ctx| {
                let center = size / 2.0;
                ctx.begin_path();
                ctx.ellipse(center, center, radius, radius);
                ctx.set_fill_style(Color::rgba(255, 255, 255, if pressed { 90 } else { 30 }));
                ctx.fill();
                ctx.set_line_width(2.0);
                ctx.set_stroke_style(Color::rgba(255, 255, 255, 110));
                ctx.stroke();
                ctx.set_font("sans-serif", if radius > 30.0 { 13.0 } else { 11.0 });
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.set_fill_style(Color::rgba(255, 255, 255, 210));
                ctx.fill_text(label, center, center);
            });
            sprite
        };
        let base = circle(JOYSTICK_RADIUS, false, "");
        let thumb = circle(THUMB_RADIUS, true, "");
        let buttons = [
            (Button::Menu, "Menu", SMALL_BUTTON_RADIUS),
            (
                Button::Jump,
                if flying { "Up" } else { "Jump" },
                BUTTON_RADIUS,
            ),
            (Button::Down, "Down", BUTTON_RADIUS),
            (
                Button::Fly,
                if flying { "Walk" } else { "Fly" },
                SMALL_BUTTON_RADIUS,
            ),
            (Button::Night, "Night", SMALL_BUTTON_RADIUS),
        ]
        .into_iter()
        .map(|(button, label, radius)| {
            (
                button,
                circle(radius, false, label),
                circle(radius, true, label),
            )
        })
        .collect();
        self.sprites = Some(Sprites {
            scale,
            flying,
            base,
            thumb,
            buttons,
        });
    }

    pub(crate) fn draw(
        &self,
        hud: &Hud,
        pass: &mut bwebgpu::RenderPass<'_>,
        layout: &Layout,
        target: (u32, u32),
    ) {
        let Some(sprites) = &self.sprites else {
            return;
        };
        let scale = sprites.scale;
        // Sprites are one logical pixel larger than their circle on each side.
        let at = |center: LogicalPoint, radius: f32| {
            (
                (center.x - radius - 1.0) * scale,
                (center.y - radius - 1.0) * scale,
            )
        };
        let center = self
            .joystick
            .as_ref()
            .map_or(layout.joystick_rest, |joystick| joystick.origin);
        hud.draw(pass, &sprites.base, at(center, JOYSTICK_RADIUS), target);
        let (dx, dy) = self.thumb();
        let thumb = LogicalPoint::new(center.x + dx, center.y + dy);
        hud.draw(pass, &sprites.thumb, at(thumb, THUMB_RADIUS), target);
        for &(button, center, radius) in &layout.buttons {
            let Some((_, normal, pressed)) = sprites.buttons.iter().find(|(b, ..)| *b == button)
            else {
                continue;
            };
            let held = self.held.iter().any(|&(_, b)| b == button);
            hud.draw(
                pass,
                if held { pressed } else { normal },
                at(center, radius),
                target,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn touch(identifier: u64, x: f32, y: f32) -> Touch {
        Touch {
            identifier,
            position: LogicalPoint::new(x, y),
        }
    }

    fn now() -> Instant {
        Instant::now()
    }

    fn layout() -> Layout {
        Layout::new(
            LogicalSize::new(800.0, 400.0),
            LogicalInsets::default(),
            false,
        )
    }

    #[test]
    fn left_side_floats_a_clamped_joystick() {
        let layout = layout();
        let mut controls = TouchControls::default();
        assert!(
            controls
                .start(&[touch(1, 300.0, 200.0)], &layout, now())
                .is_empty()
        );
        assert_eq!(controls.stick(), [0.0, 0.0]);
        controls.moved(&[touch(1, 300.0, 178.0)]);
        assert_eq!(controls.stick(), [0.0, 0.5]);
        controls.moved(&[touch(1, 400.0, 200.0)]);
        assert_eq!(controls.stick(), [1.0, 0.0]);
        controls.end(&[touch(1, 400.0, 200.0)], now());
        assert_eq!(controls.stick(), [0.0, 0.0]);
    }

    #[test]
    fn right_side_looks_while_the_joystick_moves() {
        let layout = layout();
        let mut controls = TouchControls::default();
        controls.start(
            &[touch(1, 300.0, 200.0), touch(2, 500.0, 100.0)],
            &layout,
            now(),
        );
        let actions = controls.moved(&[touch(1, 300.0, 156.0), touch(2, 510.0, 95.0)]);
        assert_eq!(actions, [Action::Look(10.0, -5.0)]);
        assert_eq!(controls.stick(), [0.0, 1.0]);
    }

    #[test]
    fn buttons_press_until_their_touch_ends() {
        let flying = Layout::new(
            LogicalSize::new(800.0, 400.0),
            LogicalInsets::default(),
            true,
        );
        let mut controls = TouchControls::default();
        // While flying Up is stacked above Down in the bottom right corner.
        let corner = 800.0 - BUTTON_MARGIN - BUTTON_RADIUS;
        let up = touch(1, corner, 400.0 - 62.0 - BUTTON_SPACING);
        let down = touch(2, corner, 400.0 - 62.0);
        let actions = controls.start(&[up, down], &flying, now());
        assert_eq!(
            actions,
            [Action::Press(Button::Jump), Action::Press(Button::Down)]
        );
        assert_eq!(controls.end(&[up], now()), [Action::Release(Button::Jump)]);
        assert_eq!(controls.reset(), [Action::Release(Button::Down)]);
        // Walking, Jump takes the corner and the Up spot looks around.
        let mut controls = TouchControls::default();
        assert_eq!(
            controls.start(&[down], &layout(), now()),
            [Action::Press(Button::Jump)]
        );
        assert!(controls.start(&[up], &layout(), now()).is_empty());
        assert!(controls.look.is_some());
    }

    #[test]
    fn hotbar_band_clears_the_joystick_and_buttons() {
        let layout = Layout::new(
            LogicalSize::new(874.0, 402.0),
            LogicalInsets::new(0.0, 62.0, 21.0, 62.0),
            true,
        );
        let (left, right) = layout.hotbar_band();
        assert!(left >= layout.joystick_rest.x + JOYSTICK_RADIUS);
        for &(_, center, radius) in &layout.buttons {
            if center.y > 402.0 / 2.0 {
                assert!(center.x + radius <= left || center.x - radius >= right);
            }
        }
    }

    #[test]
    fn canceled_touches_release_buttons_without_placing_blocks() {
        let layout = layout();
        let mut controls = TouchControls::default();
        let look = touch(1, 500.0, 150.0);
        let button = touch(2, 800.0 - BUTTON_MARGIN - BUTTON_RADIUS, 400.0 - 62.0);
        controls.start(&[look, button], &layout, now());
        assert!(controls.cancel(&[look]).is_empty());
        assert_eq!(controls.cancel(&[button]), [Action::Release(Button::Jump)]);
        assert!(controls.update(now() + LONG_PRESS).is_empty());
        assert!(controls.held.is_empty());
    }

    #[test]
    fn resetting_controls_stops_movement_and_block_actions_from_existing_touches() {
        let layout = layout();
        let mut controls = TouchControls::default();
        let start = now();
        let joystick = touch(1, 100.0, 200.0);
        let look = touch(2, 500.0, 150.0);
        let jump = touch(3, 738.0, 338.0);
        controls.start(&[joystick, look, jump], &layout, start);
        controls.moved(&[touch(1, 100.0, 150.0)]);
        assert_ne!(controls.stick(), [0.0; 2]);
        assert_eq!(controls.update(start + LONG_PRESS), [Action::Break]);

        assert_eq!(controls.reset(), [Action::Release(Button::Jump)]);
        assert_eq!(controls.stick(), [0.0; 2]);
        assert!(
            controls
                .update(start + LONG_PRESS + BREAK_REPEAT)
                .is_empty()
        );
        assert!(controls.moved(&[touch(2, 530.0, 150.0)]).is_empty());
        assert!(controls.end(&[joystick, look, jump], start).is_empty());
    }

    #[test]
    fn tapping_places_and_holding_breaks_like_pocket_edition() {
        let layout = layout();
        let start = Instant::now();
        let at = |millis| start + Duration::from_millis(millis);
        let world = touch(1, 500.0, 150.0);

        // A short tap places a block.
        let mut controls = TouchControls::default();
        controls.start(&[world], &layout, at(0));
        assert!(controls.update(at(100)).is_empty());
        assert_eq!(controls.end(&[world], at(150)), [Action::Place]);

        // Holding still breaks after the long press and repeats, without placing.
        controls.start(&[world], &layout, at(1000));
        assert_eq!(controls.update(at(1400)), [Action::Break]);
        assert!(controls.update(at(1500)).is_empty());
        assert_eq!(controls.update(at(1750)), [Action::Break]);
        assert!(controls.end(&[world], at(1800)).is_empty());

        // A drag only looks around.
        controls.start(&[world], &layout, at(2000));
        controls.moved(&[touch(1, 530.0, 150.0)]);
        assert!(controls.update(at(2600)).is_empty());
        assert!(controls.end(&[touch(1, 530.0, 150.0)], at(2700)).is_empty());
    }
}
