/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Main and settings pages over the dimmed world, with shared buttons and sliders.

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    Main,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Play,
    Settings,
    Distance,
    Stats,
    Reset,
    Back,
}

pub(crate) struct Layout {
    content: (f32, f32),
    height: f32,
    footer_y: f32,
    controls: bool,
    pub(crate) scale: f32,
    page: Page,
}

impl Layout {
    pub(crate) fn new(size: LogicalSize, safe: LogicalInsets, controls: bool, page: Page) -> Self {
        let height = if page == Page::Settings {
            330.0
        } else {
            HEADER_HEIGHT
                + BUTTON_HEIGHT * 2.0
                + 10.0
                + if controls {
                    MARGIN * 1.5 + CONTROLS.len() as f32 * LINE_HEIGHT
                } else {
                    0.0
                }
        };
        let width = (size.width - safe.left - safe.right - 24.0).max(1.0);
        let available = (size.height - safe.top - safe.bottom - 60.0).max(1.0);
        let scale = (width / CONTENT_WIDTH).min(available / height).min(1.0);
        Self {
            content: (
                safe.left + (width + 24.0 - CONTENT_WIDTH * scale) / 2.0,
                safe.top + (available - height * scale) / 2.0,
            ),
            height,
            footer_y: size.height - safe.bottom - 32.0,
            controls,
            scale,
            page,
        }
    }

    pub(crate) fn local(&self, position: LogicalPoint) -> LogicalPoint {
        LogicalPoint::new(
            (position.x - self.content.0) / self.scale,
            (position.y - self.content.1) / self.scale,
        )
    }

    pub(crate) fn action_at(&self, position: LogicalPoint) -> Option<Action> {
        let point = self.local(position);
        self.widgets().iter().find_map(|&(action, y)| {
            let width = if action == Action::Distance {
                360.0
            } else {
                BUTTON_WIDTH
            };
            let x = (CONTENT_WIDTH - width) / 2.0;
            ((x..x + width).contains(&point.x) && (y..y + BUTTON_HEIGHT).contains(&point.y))
                .then_some(action)
        })
    }

    fn widgets(&self) -> &'static [(Action, f32)] {
        if self.page == Page::Main {
            &[
                (Action::Play, HEADER_HEIGHT),
                (Action::Settings, HEADER_HEIGHT + 50.0),
            ]
        } else {
            &[
                (Action::Distance, 80.0),
                (Action::Stats, 160.0),
                (Action::Reset, 220.0),
                (Action::Back, 270.0),
            ]
        }
    }

    pub(crate) fn slider_value(&self, position: LogicalPoint) -> u8 {
        crate::settings::distance_at((self.local(position).x - 40.0) / 360.0)
    }
}

#[derive(Default)]
pub(crate) struct Menu {
    overlay: Option<Cached>,
    content: Option<Cached>,
    footer: Option<Cached>,
    pub(crate) page: Page,
    pub(crate) dragging: Option<Action>,
    pub(crate) touch_id: Option<u64>,
}

impl Menu {
    pub(crate) fn prepare(
        &mut self,
        painter: &Painter<'_>,
        layout: &Layout,
        pointer: Option<LogicalPoint>,
        settings: crate::settings::Settings,
    ) {
        painter.paint(&mut self.overlay, [0; 5], (4.0, 4.0), |ctx| {
            ctx.set_fill_style(Color::rgba(0, 0, 0, 170));
            ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
        });
        let painter = Painter {
            scale: painter.scale * layout.scale,
            ..*painter
        };
        let hovered = pointer.and_then(|point| layout.action_at(point));
        painter.paint(
            &mut self.content,
            [
                layout.height.to_bits(),
                layout.page as u32,
                hovered.map_or(0, |action| action as u32 + 1),
                u32::from(settings.distance),
                u32::from(settings.show_stats),
            ],
            (CONTENT_WIDTH, layout.height),
            |ctx| draw_content(ctx, layout, hovered, settings),
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
            hud.draw(
                pass,
                content.sprite(),
                (layout.content.0 * scale, layout.content.1 * scale),
                target,
            );
        }
        if let Some(footer) = &self.footer {
            hud.draw(
                pass,
                footer.sprite(),
                (layout.content.0 * scale, layout.footer_y * scale),
                target,
            );
        }
    }
}

fn draw_content(
    ctx: &mut CanvasRenderingContext2d<'_>,
    layout: &Layout,
    hovered: Option<Action>,
    settings: crate::settings::Settings,
) {
    let center = CONTENT_WIDTH / 2.0;
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_font_weight(FontWeight::Bold);
    ctx.set_font(
        "sans-serif",
        if layout.page == Page::Main {
            48.0
        } else {
            32.0
        },
    );
    let title = if layout.page == Page::Main {
        TITLE
    } else {
        "Settings"
    };
    ctx.set_fill_style(Color::rgba(0, 0, 0, 140));
    ctx.fill_text(title, center + 2.0, 30.0);
    ctx.set_fill_style(Color::rgb(255, 255, 255));
    ctx.fill_text(title, center, 28.0);
    ctx.set_font_weight(FontWeight::Normal);
    ctx.set_font("sans-serif", 15.0);
    if layout.page == Page::Main {
        ctx.fill_text(DESCRIPTION, center, 70.0);
    } else {
        ctx.fill_text(
            "Changes apply immediately and save automatically",
            center,
            57.0,
        );
    }
    for &(action, y) in layout.widgets() {
        let label = match action {
            Action::Play => "Play",
            Action::Settings => "Settings",
            Action::Stats => {
                if settings.show_stats {
                    "Show Debug Info: ON"
                } else {
                    "Show Debug Info: OFF"
                }
            }
            Action::Reset => "Reset to Defaults",
            Action::Back => "Done",
            Action::Distance => {
                let value = settings.distance;
                let label = format!("Render Distance: {value} chunks");
                let help = "Lower uses less memory and loads fewer chunks";
                ctx.set_fill_style(Color::rgb(255, 255, 255));
                ctx.fill_text(&label, center, y + 4.0);
                ctx.set_fill_style(Color::rgb(45, 45, 45));
                ctx.fill_rect(40.0, y + 22.0, 360.0, 8.0);
                let fraction = crate::settings::distance_fraction(value);
                ctx.set_fill_style(if hovered == Some(action) {
                    Color::rgb(255, 255, 160)
                } else {
                    Color::rgb(160, 170, 220)
                });
                ctx.fill_rect(40.0, y + 22.0, fraction * 360.0, 8.0);
                ctx.fill_rect(40.0 + fraction * 348.0, y + 17.0, 12.0, 18.0);
                ctx.set_font("sans-serif", 13.0);
                ctx.set_fill_style(Color::rgba(255, 255, 255, 170));
                ctx.fill_text(help, center, y + 53.0);
                ctx.set_font("sans-serif", 15.0);
                continue;
            }
        };
        draw_button(ctx, y, label, hovered == Some(action));
    }
    if layout.page == Page::Main && layout.controls {
        ctx.set_font("sans-serif", 14.0);
        ctx.set_fill_style(Color::rgba(255, 255, 255, 170));
        let top = HEADER_HEIGHT + BUTTON_HEIGHT * 2.0 + 10.0 + MARGIN * 1.5;
        for (index, line) in CONTROLS.iter().enumerate() {
            ctx.fill_text(line, center, top + (index as f32 + 0.5) * LINE_HEIGHT);
        }
    }
}

fn draw_button(ctx: &mut CanvasRenderingContext2d<'_>, y: f32, label: &str, hovered: bool) {
    let x = (CONTENT_WIDTH - BUTTON_WIDTH) / 2.0;
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
    let (center, label_y) = (CONTENT_WIDTH / 2.0, y + BUTTON_HEIGHT / 2.0);
    ctx.set_fill_style(Color::rgba(0, 0, 0, 150));
    ctx.fill_text(label, center + 1.0, label_y + 1.0);
    ctx.set_fill_style(if hovered {
        Color::rgb(255, 255, 160)
    } else {
        Color::rgb(255, 255, 255)
    });
    ctx.fill_text(label, center, label_y);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widgets_and_sliders_follow_scaled_safe_area_layout() {
        for size in [
            LogicalSize::new(1000.0, 600.0),
            LogicalSize::new(320.0, 480.0),
            LogicalSize::new(700.0, 300.0),
        ] {
            for page in [Page::Main, Page::Settings] {
                let layout = Layout::new(size, LogicalInsets::default(), true, page);
                assert!(layout.content.1 + layout.height * layout.scale <= layout.footer_y);
                for &(action, y) in layout.widgets() {
                    let point = LogicalPoint::new(
                        layout.content.0 + CONTENT_WIDTH / 2.0 * layout.scale,
                        layout.content.1 + (y + 20.0) * layout.scale,
                    );
                    assert_eq!(layout.action_at(point), Some(action));
                }
                let point = LogicalPoint::new(layout.content.0 - 100.0, 0.0);
                assert_eq!(layout.slider_value(point), 4);
                assert_eq!(
                    layout.slider_value(LogicalPoint::new(size.width + 100.0, 0.0)),
                    128
                );
                let point =
                    LogicalPoint::new(layout.content.0 + (40.0 + 360.0 * 0.75) * layout.scale, 0.0);
                assert_eq!(layout.slider_value(point), 64);
            }
        }
    }
}
