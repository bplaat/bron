/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Stacks three canvas layers in one window: a background that is drawn once, a tall
//! semi-transparent page that the native scroller of the window scrolls by moving its layer
//! without redrawing it, and a HUD on top that shows the scroll offset and counts the redraws
//! of every layer.

use bcanvas::{CanvasBuilder, Color, TextBaseline};
use bwindow::{
    Event, EventLoopBuilder, LogicalPoint, LogicalRect, LogicalSize, WindowBuilder, WindowEvent,
};

const PAGE_WIDTH: f32 = 240.0;
const PAGE_HEIGHT: f32 = 1200.0;
const PAGE_TOP: f32 = 120.0;
const LINE_HEIGHT: f32 = 30.0;

fn main() {
    let event_loop = EventLoopBuilder::new().build();
    let mut window = WindowBuilder::new()
        .title("Canvas Layers")
        .size(LogicalSize::new(640.0, 480.0))
        .build();
    let window_id = window.id();
    // Layers stack in the order they are added.
    let mut background = CanvasBuilder::new(&window.add_layer()).build();
    let page_layer = window.add_layer();
    let mut page = CanvasBuilder::new(&page_layer).build();
    let mut hud = CanvasBuilder::new(&window.add_layer()).build();

    // The native scroller scrolls the page, which ends a margin below the window.
    window.set_scroll_size(Some(LogicalSize::new(0.0, PAGE_TOP + PAGE_HEIGHT + 40.0)));
    let place_page = move |offset: LogicalPoint| {
        page_layer.set_frame(Some(LogicalRect::new(
            340.0,
            PAGE_TOP - offset.y,
            PAGE_WIDTH,
            PAGE_HEIGHT,
        )));
    };
    place_page(LogicalPoint::new(0.0, 0.0));

    let mut offset = LogicalPoint::new(0.0, 0.0);
    let mut redraws = [0u32; 3];
    window.request_redraw();
    event_loop.run(move |event| {
        let Event::Window(id, event) = event else {
            return;
        };
        if id != window_id {
            return;
        }
        if let WindowEvent::Scroll(scrolled) = event {
            // Moving the page layer needs no redraw of it.
            offset = scrolled;
            place_page(offset);
            hud.request_redraw();
            return;
        }
        if !matches!(event, WindowEvent::RedrawRequested) {
            return;
        }
        // Only the layers that need it get a frame.
        if background.draw(|ctx| {
            ctx.set_fill_style(Color::rgb(20, 40, 120));
            ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
            ctx.set_fill_style(Color::rgb(255, 255, 255));
            ctx.set_font("sans-serif", 28.0);
            ctx.fill_text("Background layer", 20.0, ctx.height() - 32.0);
        }) {
            redraws[0] += 1;
        }
        if page.draw(|ctx| {
            ctx.set_fill_style(Color::rgba(40, 200, 90, 170));
            ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
            ctx.set_fill_style(Color::rgb(0, 0, 0));
            ctx.set_font("sans-serif", 18.0);
            ctx.set_text_baseline(TextBaseline::Top);
            for line in 0..(PAGE_HEIGHT / LINE_HEIGHT) as u32 {
                ctx.fill_text(
                    format!("Page line {line}"),
                    12.0,
                    8.0 + line as f32 * LINE_HEIGHT,
                );
            }
        }) {
            redraws[1] += 1;
        }
        if hud.draw(|ctx| {
            ctx.clear_rect(0.0, 0.0, ctx.width(), ctx.height());
            ctx.set_fill_style(Color::rgba(220, 40, 40, 220));
            ctx.fill_rect(20.0, 20.0, 300.0, 108.0);
            ctx.set_fill_style(Color::rgb(255, 255, 255));
            ctx.set_font("sans-serif", 18.0);
            ctx.set_text_baseline(TextBaseline::Top);
            let [background, page, hud] = redraws;
            ctx.fill_text(format!("Background redraws: {background}"), 32.0, 30.0);
            ctx.fill_text(format!("Page redraws: {page}"), 32.0, 54.0);
            ctx.fill_text(format!("HUD redraws: {hud}"), 32.0, 78.0);
            ctx.fill_text(format!("Scroll offset: {:.0}", offset.y), 32.0, 102.0);
        }) {
            redraws[2] += 1;
        }
    });
}
