// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod controls;
mod game;
mod gpu;
mod scene;
mod sound;

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use bwindow::{
    Event, EventLoopBuilder, KeyCode, LogicalPoint, LogicalSize, MouseButton, WindowBuilder,
    WindowEvent,
};
use game::{Game, State};
use preferences::{Preferences, Value};

fn activate(game: &mut Game) {
    match game.state {
        State::Ready => game.start(),
        State::Dead => game.restart(),
        State::Paused => game.toggle_pause(),
        State::Playing => game.move_by(0, 1),
        State::Dying => {}
    }
}

/// Whether the position is on the top bar button that ends `right` units from the edge
fn on_button(position: LogicalPoint, size: LogicalSize, top: f32, right: f32) -> bool {
    let unit = (size.width / 400.0).clamp(0.75, 1.5);
    position.x >= size.width - (right + 32.0) * unit
        && position.x <= size.width - right * unit
        && position.y >= top + 12.0 * unit
        && position.y <= top + 62.0 * unit
}

fn press(game: &mut Game, position: LogicalPoint, size: LogicalSize, top: f32) {
    if on_button(position, size, top, 12.0) {
        game.muted = !game.muted;
        return;
    }
    if game.state != State::Playing {
        activate(game);
        return;
    }
    if on_button(position, size, top, 50.0) {
        game.toggle_pause();
        return;
    }
    game.move_by(0, 1);
}

fn gesture(game: &mut Game, start: LogicalPoint, end: LogicalPoint, size: LogicalSize, top: f32) {
    if game.state == State::Playing
        && let Some((dx, dy)) = controls::swipe(start, end)
    {
        game.move_by(dx, dy);
    } else {
        press(game, end, size, top);
    }
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .app_id("nl", "bplaat", "frogger")
        .build();
    let builder = WindowBuilder::new()
        .title("Frogger")
        .size(LogicalSize::new(640.0, 820.0))
        .min_size(LogicalSize::new(320.0, 480.0))
        .background_color(0xc7dfb5)
        .center()
        .remember_window_state();
    #[cfg(target_os = "android")]
    let builder = builder.android_system_bar_icons(bwindow::AndroidSystemBarIcons::Light);
    let window = builder.build();
    let id = window.id();
    let mut gpu = gpu::Gpu::new(window.attach_content().expect("Attach game surface"));
    let prefs = Preferences::new("nl.bplaat.frogger").expect("Open game preferences");
    let best = match prefs.get("best_score") {
        Ok(Some(Value::Integer(score))) => score.clamp(0, i64::from(i32::MAX)) as i32,
        _ => 0,
    };
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u32;
    let mut game = Game::new(best, seed);
    game.muted = matches!(prefs.get("muted"), Ok(Some(Value::Bool(true))));
    let mut saved_muted = game.muted;
    let mut sounds = sound::Sounds::new();
    sounds.set_muted(game.muted);
    let mut saved_best = best;
    let mut previous = Instant::now();
    let mut mobile = cfg!(any(target_os = "ios", target_os = "android"));
    let mut mouse = None;
    let mut scroll = controls::Scroll::default();
    let mut touch: Option<(u64, LogicalPoint)> = None;
    window.request_animation_frame();
    event_loop.run(move |event| match event {
        Event::Window(window_id, event) if window_id == id => match event {
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                game.update((now - previous).as_secs_f32());
                previous = now;
                if saved_muted != game.muted {
                    sounds.set_muted(game.muted);
                    match prefs.set("muted", game.muted) {
                        Ok(()) => saved_muted = game.muted,
                        Err(error) => eprintln!("Save mute setting: {error}"),
                    }
                }
                for cue in game.cues.drain(..) {
                    sounds.play(cue);
                }
                if saved_best != game.best {
                    match prefs.set("best_score", i64::from(game.best)) {
                        Ok(()) => saved_best = game.best,
                        Err(error) => eprintln!("Save best score: {error}"),
                    }
                }
                let size = window.size();
                let safe = window.safe_area();
                if size.width > 0.0 && size.height > 0.0 {
                    let scene = scene::Scene::new(size.width, size.height, &game, safe, mobile);
                    if gpu.render(&scene.vertices)
                        && matches!(game.state, State::Playing | State::Dying)
                    {
                        window.request_animation_frame();
                    }
                }
            }
            WindowEvent::Resize(_) | WindowEvent::ScaleFactorChanged(_) => {
                gpu.resize();
                previous = Instant::now();
                window.request_redraw();
            }
            WindowEvent::SafeAreaChanged(_) => window.request_redraw(),
            WindowEvent::Blur => {
                if game.state == State::Playing {
                    game.toggle_pause();
                }
                touch = None;
                mouse = None;
                scroll = controls::Scroll::default();
                window.request_redraw();
            }
            WindowEvent::KeyDown(event) => {
                if !event.repeat
                    || matches!(
                        event.code,
                        Some(
                            KeyCode::ArrowUp
                                | KeyCode::ArrowDown
                                | KeyCode::ArrowLeft
                                | KeyCode::ArrowRight
                                | KeyCode::KeyW
                                | KeyCode::KeyA
                                | KeyCode::KeyS
                                | KeyCode::KeyD
                        )
                    )
                {
                    let was_animating = matches!(game.state, State::Playing | State::Dying);
                    match event.code {
                        Some(KeyCode::Space | KeyCode::Enter) => activate(&mut game),
                        Some(KeyCode::KeyP | KeyCode::Escape) => game.toggle_pause(),
                        Some(KeyCode::KeyM) => game.muted = !game.muted,
                        Some(KeyCode::KeyR) if game.state != State::Dying => game.restart(),
                        Some(key) => {
                            let direction = match key {
                                KeyCode::ArrowUp | KeyCode::KeyW => Some((0, 1)),
                                KeyCode::ArrowDown | KeyCode::KeyS => Some((0, -1)),
                                KeyCode::ArrowLeft | KeyCode::KeyA => Some((-1, 0)),
                                KeyCode::ArrowRight | KeyCode::KeyD => Some((1, 0)),
                                _ => None,
                            };
                            if let Some((dx, dy)) = direction {
                                game.start();
                                game.move_by(dx, dy);
                            }
                        }
                        _ => {}
                    }
                    if !was_animating {
                        previous = Instant::now();
                    }
                    window.request_redraw();
                }
            }
            WindowEvent::MouseDown(event) if event.button == MouseButton::Left => {
                mouse = Some(event.position);
            }
            WindowEvent::MouseUp(event) if event.button == MouseButton::Left => {
                if let Some(start) = mouse.take() {
                    let was_animating = matches!(game.state, State::Playing | State::Dying);
                    gesture(
                        &mut game,
                        start,
                        event.position,
                        window.size(),
                        window.safe_area().top,
                    );
                    if !was_animating {
                        previous = Instant::now();
                    }
                    window.request_redraw();
                }
            }
            WindowEvent::MouseLeave => mouse = None,
            WindowEvent::Wheel(delta, _) => {
                if game.state == State::Playing {
                    if let Some((dx, dy)) = scroll.update(delta, Instant::now()) {
                        game.move_by(dx, dy);
                        window.request_redraw();
                    }
                } else {
                    scroll = controls::Scroll::default();
                }
            }
            WindowEvent::TouchStart(event) => {
                event.prevent_default();
                mouse = None;
                mobile = true;
                window.request_redraw();
                if touch.is_none()
                    && let Some(first) = event.changed_touches.first()
                {
                    touch = Some((first.identifier, first.position));
                }
            }
            WindowEvent::TouchEnd(event) => {
                if let Some((identifier, start)) = touch
                    && let Some(end) = event
                        .changed_touches
                        .iter()
                        .find(|t| t.identifier == identifier)
                {
                    let was_animating = matches!(game.state, State::Playing | State::Dying);
                    gesture(
                        &mut game,
                        start,
                        end.position,
                        window.size(),
                        window.safe_area().top,
                    );
                    touch = None;
                    if !was_animating {
                        previous = Instant::now();
                    }
                    window.request_redraw();
                }
            }
            WindowEvent::TouchCancel(_) => touch = None,
            _ => {}
        },
        _ => {}
    });
}
