/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The main menu over the dimmed world, like the original desktop PlaatCraft: the title,
//! a play button, the controls and a footer.

use bcanvas::{CanvasRenderingContext2d, Color, FontWeight, TextAlign, TextBaseline};
use bwindow::{LogicalInsets, LogicalPoint, LogicalSize};

use crate::hud::{Cached, Hud, MARGIN, Painter};

const TITLE: &str = "PlaatCraft";
const DESCRIPTION: &str = "A Minecraft like voxel sandbox game";
const FOOTER: &str = "Made by Bastiaan van der Plaat";
const CONTROLS: [&str; 5] = [
    "Press Esc to pause and return to this menu",
    "WASD to move, the mouse to look and Space to jump",
    "Left click breaks, right click places and middle click picks",
    "1 - 9 or the wheel select a block, E opens the inventory",
    "F toggles flying and N toggles day and night",
];
const CONTENT_WIDTH: f32 = 440.0;
const BUTTON_WIDTH: f32 = 200.0;
const BUTTON_HEIGHT: f32 = 40.0;
// The tagline ends near 79, so the play button gets the same gap as the controls below it.
const HEADER_HEIGHT: f32 = 106.0;
const LINE_HEIGHT: f32 = 20.0;
const FOOTER_HEIGHT: f32 = 24.0;

// The positions of the menu parts in logical pixels.
pub(crate) struct Layout {
    content: (f32, f32),
    height: f32,
    footer_y: f32,
    // Desktops list the controls, touch screens show their controls on screen.
    controls: bool,
}

impl Layout {
    pub(crate) fn new(size: LogicalSize, safe_area: LogicalInsets, controls: bool) -> Self {
        let height = HEADER_HEIGHT
            + BUTTON_HEIGHT
            + if controls {
                MARGIN * 1.5 + CONTROLS.len() as f32 * LINE_HEIGHT
            } else {
                0.0
            };
        let footer_y = size.height - safe_area.bottom - MARGIN - FOOTER_HEIGHT;
        Self {
            content: (
                ((size.width - CONTENT_WIDTH) / 2.0).round(),
                ((footer_y - height) / 2.0).max(safe_area.top).round(),
            ),
            height,
            footer_y,
            controls,
        }
    }

    pub(crate) fn button_at(&self, position: LogicalPoint) -> bool {
        let x = self.content.0 + (CONTENT_WIDTH - BUTTON_WIDTH) / 2.0;
        let y = self.content.1 + HEADER_HEIGHT;
        (x..x + BUTTON_WIDTH).contains(&position.x) && (y..y + BUTTON_HEIGHT).contains(&position.y)
    }
}

#[derive(Default)]
pub(crate) struct Menu {
    overlay: Option<Cached>,
    content: Option<Cached>,
    footer: Option<Cached>,
}

impl Menu {
    // Draws the sprites again when the scale or the hovered button changed.
    pub(crate) fn prepare(&mut self, painter: &Painter<'_>, layout: &Layout, hovered: bool) {
        painter.paint(&mut self.overlay, [0; 5], (4.0, 4.0), |ctx| {
            ctx.set_fill_style(Color::rgba(0, 0, 0, 170));
            ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
        });
        let controls = layout.controls;
        painter.paint(
            &mut self.content,
            [layout.height.to_bits(), 0, u32::from(hovered), 0, 0],
            (CONTENT_WIDTH, layout.height),
            |ctx| draw_content(ctx, controls, hovered),
        );
        painter.paint(
            &mut self.footer,
            [0; 5],
            (CONTENT_WIDTH, FOOTER_HEIGHT),
            |ctx| {
                ctx.set_font("sans-serif", 15.0);
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.set_fill_style(Color::rgba(255, 255, 255, 170));
                ctx.fill_text(FOOTER, ctx.width() / 2.0, ctx.height() / 2.0);
            },
        );
    }

    pub(crate) fn draw(
        &self,
        hud: &Hud,
        pass: &mut bwebgpu::RenderPass<'_>,
        layout: &Layout,
        scale: f32,
        target: (u32, u32),
    ) {
        if let Some(overlay) = &self.overlay {
            hud.fill(pass, overlay.sprite(), target);
        }
        if let Some(content) = &self.content {
            let (x, y) = layout.content;
            hud.draw(pass, content.sprite(), (x * scale, y * scale), target);
        }
        if let Some(footer) = &self.footer {
            let position = (layout.content.0 * scale, layout.footer_y * scale);
            hud.draw(pass, footer.sprite(), position, target);
        }
    }
}

fn draw_content(ctx: &mut CanvasRenderingContext2d<'_>, controls: bool, hovered: bool) {
    let center = CONTENT_WIDTH / 2.0;
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_font_weight(FontWeight::Bold);
    ctx.set_font("sans-serif", 48.0);
    ctx.set_fill_style(Color::rgba(0, 0, 0, 140));
    ctx.fill_text(TITLE, center + 2.0, 30.0);
    ctx.set_fill_style(Color::rgb(255, 255, 255));
    ctx.fill_text(TITLE, center, 28.0);
    ctx.set_font_weight(FontWeight::Normal);
    ctx.set_font("sans-serif", 17.0);
    ctx.set_fill_style(Color::rgba(255, 255, 255, 210));
    ctx.fill_text(DESCRIPTION, center, 70.0);

    // A raised Minecraft style button, lighter while hovered.
    let (x, y) = ((CONTENT_WIDTH - BUTTON_WIDTH) / 2.0, HEADER_HEIGHT);
    ctx.set_fill_style(Color::rgb(0, 0, 0));
    ctx.fill_rect(x, y, BUTTON_WIDTH, BUTTON_HEIGHT);
    ctx.set_fill_style(if hovered {
        Color::rgb(126, 136, 191)
    } else {
        Color::rgb(111, 111, 111)
    });
    ctx.fill_rect(x + 2.0, y + 2.0, BUTTON_WIDTH - 4.0, BUTTON_HEIGHT - 4.0);
    ctx.set_fill_style(Color::rgba(255, 255, 255, 90));
    ctx.fill_rect(x + 2.0, y + 2.0, BUTTON_WIDTH - 4.0, 2.0);
    ctx.fill_rect(x + 2.0, y + 2.0, 2.0, BUTTON_HEIGHT - 4.0);
    ctx.set_fill_style(Color::rgba(0, 0, 0, 90));
    ctx.fill_rect(x + 2.0, y + BUTTON_HEIGHT - 4.0, BUTTON_WIDTH - 4.0, 2.0);
    ctx.fill_rect(x + BUTTON_WIDTH - 4.0, y + 2.0, 2.0, BUTTON_HEIGHT - 4.0);
    ctx.set_font("sans-serif", 17.0);
    let label_y = y + BUTTON_HEIGHT / 2.0;
    ctx.set_fill_style(Color::rgba(0, 0, 0, 150));
    ctx.fill_text("Play", center + 1.0, label_y + 1.0);
    ctx.set_fill_style(if hovered {
        Color::rgb(255, 255, 160)
    } else {
        Color::rgb(255, 255, 255)
    });
    ctx.fill_text("Play", center, label_y);

    if controls {
        ctx.set_font("sans-serif", 14.0);
        ctx.set_fill_style(Color::rgba(255, 255, 255, 170));
        let top = HEADER_HEIGHT + BUTTON_HEIGHT + MARGIN * 1.5;
        for (index, line) in CONTROLS.iter().enumerate() {
            ctx.fill_text(line, center, top + (index as f32 + 0.5) * LINE_HEIGHT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_button_is_centered_above_the_footer() {
        let size = LogicalSize::new(1000.0, 600.0);
        let layout = Layout::new(size, LogicalInsets::default(), true);
        assert!(layout.content.1 + layout.height <= layout.footer_y);
        let button = LogicalPoint::new(500.0, layout.content.1 + HEADER_HEIGHT + 20.0);
        assert!(layout.button_at(button));
        assert!(!layout.button_at(LogicalPoint::new(500.0, layout.content.1)));
        assert!(!layout.button_at(LogicalPoint::new(500.0 + BUTTON_WIDTH, button.y)));
    }
}
