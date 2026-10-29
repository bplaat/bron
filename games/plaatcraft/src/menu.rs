/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The engine's menus with the mouse, touch and a keyboard, drawn with bcanvas into cached
//! textures over the dimmed world.

use std::hash::{BuildHasher, RandomState};
use std::ops::{Deref, DerefMut};
use std::sync::LazyLock;
use std::time::SystemTime;

use bcanvas::{CanvasRenderingContext2d, Color, FontWeight, TextAlign, TextBaseline};
use bwindow::{LogicalInsets, LogicalPoint, LogicalSize};
use plaatcraft_engine::menu::{
    self, Align, CONTENT_WIDTH, Canvas, FOOTER_HEIGHT, Font, Keyboard, Platform, Row,
};
pub(crate) use plaatcraft_engine::menu::{Action, Page, Session};

use crate::hud::{Cached, Hud, Painter};
use crate::lan::Found;
use crate::settings::Settings;
use crate::worlds::World;

const CONTROLS: [&str; 5] = [
    "Press Esc to pause and return to this menu",
    "WASD to move, the mouse to look and Space to jump",
    "Left click breaks, right click places and middle click picks",
    "1 - 9 or the wheel select a block, E opens the inventory",
    "T opens the chat, try /fly on and /time",
];

// A random hasher state, the same for all cache keys of a run.
static KEYS: LazyLock<RandomState> = LazyLock::new(RandomState::new);

// Desktops have a keyboard to type names and show the controls, touch screens don't.
const fn platform(keyboard: bool) -> Platform {
    Platform {
        keyboard: if keyboard {
            Keyboard::Physical
        } else {
            Keyboard::None
        },
        // iOS apps don't quit themselves.
        quits: cfg!(not(target_os = "ios")),
        distance: true,
        confirm: "Enter",
        open: if keyboard {
            "Click a world twice"
        } else {
            "Tap a world twice"
        },
        controls: if keyboard { &CONTROLS } else { &[] },
    }
}

const fn point(position: LogicalPoint) -> [f32; 2] {
    [position.x, position.y]
}

/// Where the widgets of the shown page are in the window.
pub(crate) struct Layout(menu::Layout, bool);

impl Layout {
    pub(crate) fn new(size: LogicalSize, safe: LogicalInsets, keyboard: bool, menu: &Menu) -> Self {
        let layout = menu::Layout::new(
            &menu.state,
            platform(keyboard),
            [size.width, size.height],
            [safe.top, safe.right, safe.bottom, safe.left],
        );
        Self(layout, keyboard)
    }

    pub(crate) fn action_at(&self, position: LogicalPoint) -> Option<Action> {
        self.0.action_at(point(position))
    }

    /// Whether a position is over the list of worlds.
    pub(crate) fn in_list(&self, position: LogicalPoint) -> bool {
        self.0.in_list(point(position))
    }

    /// The position along a slider, from 0.0 to 1.0
    pub(crate) fn slider_fraction(&self, position: LogicalPoint) -> f32 {
        self.0.slider_fraction(point(position))
    }
}

#[derive(Default)]
pub(crate) struct Menu {
    state: menu::Menu,
    overlay: Option<Cached>,
    content: Option<Cached>,
    footer: Option<Cached>,
    pub(crate) dragging: Option<Action>,
    pub(crate) touch_id: Option<u64>,
    worlds: Vec<World>,
    /// The worlds found on the LAN.
    servers: Vec<Found>,
}

impl Deref for Menu {
    type Target = menu::Menu;

    fn deref(&self) -> &menu::Menu {
        &self.state
    }
}

impl DerefMut for Menu {
    fn deref_mut(&mut self) -> &mut menu::Menu {
        &mut self.state
    }
}

impl Menu {
    /// Shows the worlds, with the named one or else the most recently played one selected.
    pub(crate) fn show_worlds(&mut self, worlds: Vec<World>, selected: Option<&str>) {
        let selected = selected.and_then(|name| worlds.iter().position(|world| world.name == name));
        let rows = worlds
            .iter()
            .map(|world| Row {
                title: world.name.clone(),
                details: played(world.modified),
                enabled: true,
            })
            .collect();
        self.worlds = worlds;
        self.state.show_list(Page::Worlds, rows, selected);
    }

    /// Shows the worlds on the LAN.
    pub(crate) fn show_servers(&mut self) {
        let rows = self.server_rows();
        self.state.show_list(Page::Multiplayer, rows, None);
    }

    /// Adds the newly found LAN worlds, keeping the selected one selected.
    pub(crate) fn update_servers(&mut self, found: Vec<Found>) {
        let selected = self.selected_server().map(|found| found.address);
        if crate::lan::merge(&mut self.servers, found) && self.page == Page::Multiplayer {
            let selected = selected.and_then(|address| {
                self.servers
                    .iter()
                    .position(|found| found.address == address)
            });
            let rows = self.server_rows();
            self.state.set_rows(rows, selected);
        }
    }

    fn server_rows(&self) -> Vec<Row> {
        self.servers
            .iter()
            .map(|found| {
                let world = &found.announcement;
                let details = if found.compatible {
                    let plural = if world.players == 1 { "" } else { "s" };
                    format!(
                        "{} - {} player{plural} - {}",
                        world.host, world.players, found.address
                    )
                } else {
                    format!("{} - needs another version of PlaatCraft", world.host)
                };
                Row {
                    title: world.world.clone(),
                    details,
                    enabled: found.compatible,
                }
            })
            .collect()
    }

    pub(crate) fn selected_world(&self) -> Option<&str> {
        Some(&self.worlds.get(self.selected?)?.name)
    }

    pub(crate) fn selected_server(&self) -> Option<&Found> {
        self.servers.get(self.selected?)
    }

    pub(crate) fn start_list_drag(&mut self, layout: &Layout, position: LogicalPoint) {
        self.state.start_list_drag(&layout.0, point(position));
    }

    pub(crate) fn list_drag(&mut self, layout: &Layout, position: LogicalPoint) {
        self.state.list_drag(&layout.0, point(position));
    }

    pub(crate) const fn end_drag(&mut self) {
        self.dragging = None;
        self.state.end_list_drag();
    }

    pub(crate) fn prepare(
        &mut self,
        painter: &Painter<'_>,
        layout: &Layout,
        pointer: Option<LogicalPoint>,
        settings: Settings,
    ) {
        painter.paint(&mut self.overlay, [0; 5], (4.0, 4.0), |ctx| {
            ctx.set_fill_style(Color::rgba(0, 0, 0, 170));
            ctx.fill_rect(0.0, 0.0, ctx.width(), ctx.height());
        });
        let painter = Painter {
            scale: painter.scale * layout.0.scale,
            ..*painter
        };
        let hovered = pointer.and_then(|point| layout.action_at(point));
        let hash = KEYS.hash_one((&self.state, hovered, settings));
        let mut content = self.content.take();
        painter.paint(
            &mut content,
            [
                layout.0.height.to_bits(),
                self.page as u32,
                // The hints and pages differ between a keyboard and touch.
                u32::from(layout.1),
                hash as u32,
                (hash >> 32) as u32,
            ],
            (CONTENT_WIDTH, layout.0.height),
            |ctx| {
                self.state
                    .draw_content(&mut Bcanvas(ctx), &layout.0, hovered, settings);
            },
        );
        self.content = content;
        painter.paint(
            &mut self.footer,
            [0; 5],
            (CONTENT_WIDTH, FOOTER_HEIGHT),
            |ctx| menu::draw_footer(&mut Bcanvas(ctx)),
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
        let [x, y] = layout.0.content;
        if let Some(overlay) = &self.overlay {
            hud.fill(pass, overlay.sprite(), target);
        }
        if let Some(content) = &self.content {
            hud.draw(pass, content.sprite(), (x * scale, y * scale), target);
        }
        if let Some(footer) = &self.footer {
            let position = (x * scale, layout.0.footer_y * scale);
            hud.draw(pass, footer.sprite(), position, target);
        }
    }
}

// The engine's menus drawn on a bcanvas context.
struct Bcanvas<'a, 'b>(&'a mut CanvasRenderingContext2d<'b>);

const fn color([red, green, blue, alpha]: menu::Color) -> Color {
    Color::rgba(red, green, blue, alpha)
}

impl Canvas for Bcanvas<'_, '_> {
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, fill: menu::Color) {
        self.0.set_fill_style(color(fill));
        self.0.fill_rect(x, y, width, height);
    }

    fn text(&mut self, text: &str, x: f32, y: f32, font: Font) {
        let ctx = &mut *self.0;
        ctx.set_font_weight(if font.bold {
            FontWeight::Bold
        } else {
            FontWeight::Normal
        });
        ctx.set_font("sans-serif", font.size);
        ctx.set_text_align(match font.align {
            Align::Left => TextAlign::Left,
            Align::Center => TextAlign::Center,
        });
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_style(color(font.color));
        ctx.fill_text(text, x, y);
    }
}

// When a world was last played, in words.
fn played(modified: SystemTime) -> String {
    menu::played(modified.elapsed().map_or(0, |elapsed| elapsed.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_lan_worlds_keep_their_selection() {
        let found = |host: &str, port: u16| Found {
            address: std::net::SocketAddr::from(([192, 168, 1, 5], port)),
            announcement: plaatcraft_engine::protocol::Announcement {
                port,
                world: "Island".into(),
                host: host.into(),
                players: 2,
            },
            compatible: port != 1,
            seen: std::time::Instant::now(),
        };
        let mut menu = Menu::default();
        menu.show_servers();
        assert_eq!(menu.selected, None);
        menu.update_servers(vec![found("Alice", 47820)]);
        assert_eq!(menu.selected, Some(0));
        menu.select_step(1);
        menu.update_servers(vec![found("Bob", 47900), found("Carol", 1)]);
        assert_eq!(menu.selected_server().unwrap().announcement.host, "Alice");
        menu.select_step(2);
        let layout = Layout::new(
            LogicalSize::new(1000.0, 800.0),
            LogicalInsets::default(),
            true,
            &menu,
        );
        assert!(
            !layout
                .0
                .actions()
                .any(|action| action == Action::JoinServer)
        );
        assert_eq!(
            menu.rows[0].details,
            "Alice - 2 players - 192.168.1.5:47820"
        );
    }

    #[test]
    fn worlds_show_when_they_were_last_played() {
        let mut menu = Menu::default();
        let modified = SystemTime::now() - std::time::Duration::from_secs(7200);
        let worlds = vec![World {
            name: "Island".into(),
            modified,
        }];
        menu.show_worlds(worlds, Some("Island"));
        assert_eq!(menu.selected_world(), Some("Island"));
        assert_eq!(menu.rows[0].details, "Played 2 hours ago");
    }
}
