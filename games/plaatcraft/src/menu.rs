/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The title, pause, world, multiplayer, and settings pages over the dimmed world, with
//! shared buttons, lists, text fields, and sliders.

use std::hash::{BuildHasher, Hash, RandomState};
use std::sync::LazyLock;
use std::time::SystemTime;

use bcanvas::{CanvasRenderingContext2d, Color, FontWeight, TextAlign, TextBaseline};
use bwindow::{LogicalInsets, LogicalPoint, LogicalSize};

use crate::hud::{Cached, Hud, MARGIN, Painter};
use crate::lan::Found;
use crate::server::Identity;
use crate::settings::Settings;
use crate::worlds::World;

const TITLE: &str = "PlaatCraft";
const DESCRIPTION: &str = "A Minecraft like voxel sandbox game";
const FOOTER: &str = "Made by Bastiaan van der Plaat";
const CONTROLS: [&str; 5] = [
    "Press Esc to pause and return to this menu",
    "WASD to move, the mouse to look and Space to jump",
    "Left click breaks, right click places and middle click picks",
    "1 - 9 or the wheel select a block, E opens the inventory",
    "T opens the chat, try /fly on and /time night",
];
const CONTENT_WIDTH: f32 = 440.0;
const BUTTON_WIDTH: f32 = 200.0;
const BUTTON_HEIGHT: f32 = 40.0;
const WIDE_WIDTH: f32 = 360.0;
const WIDE_X: f32 = (CONTENT_WIDTH - WIDE_WIDTH) / 2.0;
const HALF_WIDTH: f32 = 175.0;
// The tagline ends near 79, so the first button gets the same gap as the controls below it.
const HEADER_HEIGHT: f32 = 106.0;
const LINE_HEIGHT: f32 = 20.0;
const FOOTER_HEIGHT: f32 = 24.0;
const LIST_TOP: f32 = 80.0;
const LIST_ROWS: usize = 5;
const ROW_HEIGHT: f32 = 48.0;
const LIST_BOTTOM: f32 = LIST_TOP + LIST_ROWS as f32 * ROW_HEIGHT;
const FIELD_TOP: f32 = 110.0;
// iOS apps don't quit themselves, so only the other platforms get a quit button.
const QUITS: bool = cfg!(not(target_os = "ios"));

// A random hasher state, the same for all cache keys of a run.
static KEYS: LazyLock<RandomState> = LazyLock::new(RandomState::new);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Page {
    #[default]
    Title,
    Pause,
    Worlds,
    Create,
    Rename,
    Delete,
    Multiplayer,
    DirectConnect,
    Username,
    Settings,
}

impl Page {
    /// Whether the page has a text field.
    pub(crate) const fn typing(self) -> bool {
        matches!(
            self,
            Self::Create | Self::Rename | Self::DirectConnect | Self::Username
        )
    }

    const fn listing(self) -> bool {
        matches!(self, Self::Worlds | Self::Multiplayer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Action {
    SinglePlayer,
    MultiPlayer,
    Resume,
    Settings,
    Quit,
    OpenToLan,
    QuitToTitle,
    Row(usize),
    PlayWorld,
    CreateWorld,
    RenameWorld,
    DeleteWorld,
    JoinServer,
    DirectConnect,
    Confirm,
    Distance,
    Volume,
    Stats,
    AutoJump,
    Username,
    Reset,
    Back,
}

/// How the player is in a world, which changes the pause page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Session {
    #[default]
    None,
    Host {
        lan: Option<u16>,
    },
    Guest,
}

#[derive(Clone, Copy, Debug)]
struct Widget {
    action: Action,
    x: f32,
    y: f32,
    width: f32,
    enabled: bool,
}

impl Widget {
    const fn button(action: Action, y: f32) -> Self {
        Self {
            action,
            x: (CONTENT_WIDTH - BUTTON_WIDTH) / 2.0,
            y,
            width: BUTTON_WIDTH,
            enabled: true,
        }
    }

    const fn wide(action: Action, y: f32) -> Self {
        Self {
            action,
            x: WIDE_X,
            y,
            width: WIDE_WIDTH,
            enabled: true,
        }
    }

    // A button in the left or right half of a row.
    const fn half(action: Action, right: bool, y: f32, enabled: bool) -> Self {
        Self {
            action,
            x: if right {
                CONTENT_WIDTH - WIDE_X - HALF_WIDTH
            } else {
                WIDE_X
            },
            y,
            width: HALF_WIDTH,
            enabled,
        }
    }

    const fn height(&self) -> f32 {
        if matches!(self.action, Action::Row(_)) {
            ROW_HEIGHT - 4.0
        } else {
            BUTTON_HEIGHT
        }
    }
}

pub(crate) struct Layout {
    content: (f32, f32),
    height: f32,
    footer_y: f32,
    // Desktops have a keyboard to type names and show the controls.
    keyboard: bool,
    pub(crate) scale: f32,
    page: Page,
    widgets: Vec<Widget>,
}

impl Layout {
    pub(crate) fn new(size: LogicalSize, safe: LogicalInsets, keyboard: bool, menu: &Menu) -> Self {
        let page = menu.page;
        let (height, widgets) = Self::widgets(page, keyboard, menu);
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
            keyboard,
            scale,
            page,
            widgets,
        }
    }

    // The content height and the widgets of a page.
    fn widgets(page: Page, keyboard: bool, menu: &Menu) -> (f32, Vec<Widget>) {
        let main = |actions: &[(Action, bool)]| {
            let widgets: Vec<_> = actions
                .iter()
                .enumerate()
                .map(|(index, &(action, enabled))| Widget {
                    enabled,
                    ..Widget::button(action, HEADER_HEIGHT + index as f32 * 50.0)
                })
                .collect();
            let controls = if keyboard {
                MARGIN * 1.5 + CONTROLS.len() as f32 * LINE_HEIGHT
            } else {
                0.0
            };
            (main_bottom(&widgets) + controls, widgets)
        };
        let confirm = |y: f32| {
            vec![
                Widget::half(Action::Confirm, false, y, true),
                Widget::half(Action::Back, true, y, true),
            ]
        };
        let rows = |menu: &Menu| -> Vec<Widget> {
            (menu.scroll..menu.row_count())
                .take(LIST_ROWS)
                .enumerate()
                .map(|(row, index)| {
                    Widget::wide(Action::Row(index), LIST_TOP + row as f32 * ROW_HEIGHT)
                })
                .collect()
        };
        let y = LIST_BOTTOM + 16.0;
        match page {
            Page::Title => {
                let mut actions = vec![
                    (Action::SinglePlayer, true),
                    (Action::MultiPlayer, true),
                    (Action::Settings, true),
                ];
                if QUITS {
                    actions.push((Action::Quit, true));
                }
                main(&actions)
            }
            Page::Pause => {
                let mut actions = vec![(Action::Resume, true), (Action::Settings, true)];
                if let Session::Host { lan } = menu.session {
                    actions.push((Action::OpenToLan, lan.is_none()));
                }
                actions.push((Action::QuitToTitle, true));
                main(&actions)
            }
            Page::Worlds => {
                let selected = menu.selected.is_some();
                let mut widgets = rows(menu);
                widgets.push(Widget::half(Action::PlayWorld, false, y, selected));
                widgets.push(Widget::half(Action::CreateWorld, true, y, true));
                // Touch screens have no keyboard to type a new name.
                if keyboard {
                    widgets.push(Widget::half(Action::RenameWorld, false, y + 50.0, selected));
                    widgets.push(Widget::half(Action::DeleteWorld, true, y + 50.0, selected));
                } else {
                    widgets.push(Widget {
                        enabled: selected,
                        ..Widget::button(Action::DeleteWorld, y + 50.0)
                    });
                }
                widgets.push(Widget::button(Action::Back, y + 100.0));
                (y + 140.0, widgets)
            }
            Page::Multiplayer => {
                let joinable = menu.selected_server().is_some_and(|found| found.compatible);
                let mut widgets = rows(menu);
                widgets.push(Widget::half(Action::JoinServer, false, y, joinable));
                // Touch screens have no keyboard to type an address.
                if keyboard {
                    widgets.push(Widget::half(Action::DirectConnect, true, y, true));
                    widgets.push(Widget::button(Action::Back, y + 50.0));
                    (y + 90.0, widgets)
                } else {
                    widgets.push(Widget::half(Action::Back, true, y, true));
                    (y + 40.0, widgets)
                }
            }
            page if page.typing() => (FIELD_TOP + 140.0, confirm(FIELD_TOP + 100.0)),
            Page::Delete => (190.0, confirm(150.0)),
            _ => {
                let mut widgets = vec![
                    Widget::wide(Action::Distance, 80.0),
                    Widget::wide(Action::Volume, 160.0),
                    Widget::button(Action::Stats, 240.0),
                    Widget::button(Action::AutoJump, 290.0),
                ];
                // Touch screens have no keyboard to type a username.
                if keyboard {
                    widgets.push(Widget::button(Action::Username, 340.0));
                }
                let y = main_bottom(&widgets) + 10.0;
                widgets.push(Widget::button(Action::Reset, y));
                widgets.push(Widget::button(Action::Back, y + 50.0));
                (y + 100.0, widgets)
            }
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
        self.widgets.iter().find_map(|widget| {
            (widget.enabled
                && (widget.x..widget.x + widget.width).contains(&point.x)
                && (widget.y..widget.y + widget.height()).contains(&point.y))
            .then_some(widget.action)
        })
    }

    /// Whether a position is over the list of worlds.
    pub(crate) fn in_list(&self, position: LogicalPoint) -> bool {
        let point = self.local(position);
        self.page.listing()
            && (WIDE_X..WIDE_X + WIDE_WIDTH).contains(&point.x)
            && (LIST_TOP..LIST_BOTTOM).contains(&point.y)
    }

    /// The position along a slider, from 0.0 to 1.0
    pub(crate) fn slider_fraction(&self, position: LogicalPoint) -> f32 {
        ((self.local(position).x - WIDE_X) / WIDE_WIDTH).clamp(0.0, 1.0)
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
    pub(crate) worlds: Vec<World>,
    /// The worlds found on the LAN.
    pub(crate) servers: Vec<Found>,
    /// The selected row of the shown list.
    pub(crate) selected: Option<usize>,
    scroll: usize,
    // Where dragging the list started, and its scroll then.
    list_drag: Option<(f32, usize)>,
    // The typed text, and why it can't be used.
    pub(crate) name: String,
    pub(crate) error: Option<String>,
    pub(crate) identity: Option<Identity>,
    pub(crate) session: Session,
}

impl Menu {
    /// Shows the worlds, with the named one or else the most recently played one selected.
    pub(crate) fn show_worlds(&mut self, worlds: Vec<World>, selected: Option<&str>) {
        self.selected = selected
            .and_then(|name| worlds.iter().position(|world| world.name == name))
            .or_else(|| (!worlds.is_empty()).then_some(0));
        self.worlds = worlds;
        self.show_list(Page::Worlds);
    }

    /// Shows the worlds on the LAN.
    pub(crate) fn show_servers(&mut self) {
        self.selected = (!self.servers.is_empty()).then_some(0);
        self.show_list(Page::Multiplayer);
    }

    fn show_list(&mut self, page: Page) {
        self.scroll = 0;
        self.scroll_to_selected();
        self.page = page;
        self.error = None;
    }

    /// Adds the newly found LAN worlds, keeping the selected one selected.
    pub(crate) fn update_servers(&mut self, found: Vec<Found>) {
        let selected = self.selected_server().map(|found| found.address);
        if crate::lan::merge(&mut self.servers, found) && self.page == Page::Multiplayer {
            self.selected = selected
                .and_then(|address| {
                    self.servers
                        .iter()
                        .position(|found| found.address == address)
                })
                .or_else(|| (!self.servers.is_empty()).then_some(0));
            self.scroll = self.scroll.min(self.max_scroll());
        }
    }

    pub(crate) fn selected_world(&self) -> Option<&str> {
        Some(&self.worlds.get(self.selected?)?.name)
    }

    pub(crate) fn selected_server(&self) -> Option<&Found> {
        self.servers.get(self.selected?)
    }

    const fn row_count(&self) -> usize {
        match self.page {
            Page::Multiplayer => self.servers.len(),
            _ => self.worlds.len(),
        }
    }

    // The title and the line below it of each row of the shown list.
    fn rows(&self) -> Vec<(String, String)> {
        match self.page {
            Page::Multiplayer => self
                .servers
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
                    (world.world.clone(), details)
                })
                .collect(),
            _ => self
                .worlds
                .iter()
                .map(|world| (world.name.clone(), played(world.modified)))
                .collect(),
        }
    }

    const fn max_scroll(&self) -> usize {
        self.row_count().saturating_sub(LIST_ROWS)
    }

    fn scroll_to_selected(&mut self) {
        if let Some(selected) = self.selected {
            self.scroll = self
                .scroll
                .clamp(selected.saturating_sub(LIST_ROWS - 1), selected);
        }
    }

    /// Scrolls the list, positive steps scroll up.
    pub(crate) fn scroll_steps(&mut self, steps: i64) {
        self.scroll = self
            .scroll
            .saturating_add_signed(-steps as isize)
            .min(self.max_scroll());
    }

    /// Moves the selection with the arrow keys.
    pub(crate) fn select_step(&mut self, step: isize) {
        let count = self.row_count();
        if count > 0 {
            self.selected = Some(
                self.selected
                    .map_or(0, |selected| selected.saturating_add_signed(step))
                    .min(count - 1),
            );
            self.scroll_to_selected();
        }
    }

    pub(crate) fn start_list_drag(&mut self, layout: &Layout, position: LogicalPoint) {
        self.list_drag = Some((layout.local(position).y, self.scroll));
    }

    pub(crate) fn list_drag(&mut self, layout: &Layout, position: LogicalPoint) {
        if let Some((start, scroll)) = self.list_drag {
            let rows = ((start - layout.local(position).y) / ROW_HEIGHT).round() as isize;
            self.scroll = scroll.saturating_add_signed(rows).min(self.max_scroll());
        }
    }

    pub(crate) const fn end_drag(&mut self) {
        self.dragging = None;
        self.list_drag = None;
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
            scale: painter.scale * layout.scale,
            ..*painter
        };
        let hovered = pointer.and_then(|point| layout.action_at(point));
        let rows = if layout.page.listing() {
            self.rows()
        } else {
            Vec::new()
        };
        let hash = KEYS.hash_one((
            (layout.page, hovered, settings, self.session),
            (self.selected, self.scroll, &rows),
            (&self.name, &self.error),
            self.identity.as_ref().map(|identity| &identity.name),
        ));
        let mut content = self.content.take();
        painter.paint(
            &mut content,
            [
                layout.height.to_bits(),
                layout.page as u32,
                u32::from(layout.keyboard),
                hash as u32,
                (hash >> 32) as u32,
            ],
            (CONTENT_WIDTH, layout.height),
            |ctx| self.draw_content(ctx, layout, hovered, settings, &rows),
        );
        self.content = content;
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

    fn subtitle(&self, page: Page) -> String {
        let text = match page {
            Page::Title | Page::Pause => DESCRIPTION,
            _ if let Some(error) = &self.error
                && page.listing() =>
            {
                return error.clone();
            }
            Page::Worlds if self.worlds.is_empty() => "Create a world to start playing",
            Page::Worlds => "Click a world twice to play it",
            Page::Multiplayer => {
                let name = self.identity.as_ref().map_or("", |identity| &identity.name);
                return if self.servers.is_empty() {
                    format!("Playing as {name}, searching the LAN for worlds...")
                } else {
                    format!("Playing as {name}, click a world twice to join it")
                };
            }
            _ => "",
        };
        text.to_owned()
    }

    fn label(&self, page: Page, action: Action, settings: Settings) -> String {
        let label = match action {
            Action::SinglePlayer => "Single Player",
            Action::MultiPlayer => "Multi Player",
            Action::Resume => "Back to Game",
            Action::Settings => "Settings",
            Action::Quit => "Quit",
            Action::OpenToLan => match self.session {
                Session::Host { lan: Some(port) } => return format!("LAN Open on Port {port}"),
                _ => "Open to LAN",
            },
            Action::QuitToTitle if self.session == Session::Guest => "Disconnect",
            Action::QuitToTitle => "Save and Quit to Title",
            Action::PlayWorld => "Play Selected World",
            Action::CreateWorld => "Create New World",
            Action::RenameWorld => "Rename",
            Action::DeleteWorld => "Delete",
            Action::JoinServer => "Join World",
            Action::DirectConnect => "Direct Connect",
            Action::Confirm => match page {
                Page::Create => "Create",
                Page::Rename => "Rename",
                Page::DirectConnect => "Join",
                Page::Username => "Done",
                _ => "Delete",
            },
            Action::Back if page == Page::Settings => "Done",
            Action::Back if page.listing() => "Back",
            Action::Back => "Cancel",
            Action::Stats if settings.show_stats => "Show Debug Info: ON",
            Action::Stats => "Show Debug Info: OFF",
            Action::AutoJump if settings.auto_jump => "Auto Jump: ON",
            Action::AutoJump => "Auto Jump: OFF",
            Action::Username => {
                let name = self.identity.as_ref().map_or("", |identity| &identity.name);
                return format!("Username: {name}");
            }
            Action::Reset => "Reset to Defaults",
            Action::Row(_) | Action::Distance | Action::Volume => "",
        };
        label.to_owned()
    }

    fn draw_content(
        &self,
        ctx: &mut CanvasRenderingContext2d<'_>,
        layout: &Layout,
        hovered: Option<Action>,
        settings: Settings,
        rows: &[(String, String)],
    ) {
        let center = CONTENT_WIDTH / 2.0;
        let title = match layout.page {
            Page::Title => TITLE,
            Page::Pause => "Game Paused",
            Page::Worlds => "Select World",
            Page::Create => "Create New World",
            Page::Rename => "Rename World",
            Page::Delete => "Delete World",
            Page::Multiplayer => "Multi Player",
            Page::DirectConnect => "Direct Connect",
            Page::Username => "Username",
            Page::Settings => "Settings",
        };
        let main = matches!(layout.page, Page::Title | Page::Pause);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_font_weight(FontWeight::Bold);
        ctx.set_font("sans-serif", if main { 48.0 } else { 32.0 });
        ctx.set_fill_style(Color::rgba(0, 0, 0, 140));
        ctx.fill_text(title, center + 2.0, 30.0);
        ctx.set_fill_style(Color::rgb(255, 255, 255));
        ctx.fill_text(title, center, 28.0);
        ctx.set_font_weight(FontWeight::Normal);
        ctx.set_font("sans-serif", 15.0);
        ctx.fill_text(
            self.subtitle(layout.page),
            center,
            if main { 70.0 } else { 57.0 },
        );

        match layout.page {
            page if page.listing() => self.draw_list(ctx, hovered, rows),
            page if page.typing() => self.draw_field(ctx, page),
            Page::Delete => {
                let selected = self.selected_world().unwrap_or_default();
                ctx.fill_text(format!("'{selected}' will be lost forever!"), center, 90.0);
                ctx.set_fill_style(Color::rgba(255, 255, 255, 170));
                ctx.fill_text("This can't be undone", center, 115.0);
            }
            _ => {}
        }

        for widget in &layout.widgets {
            match widget.action {
                Action::Row(_) => {}
                Action::Distance | Action::Volume => {
                    draw_slider(ctx, widget, hovered == Some(widget.action), settings);
                }
                action => draw_button(
                    ctx,
                    widget,
                    &self.label(layout.page, action, settings),
                    hovered == Some(action),
                ),
            }
        }
        if main && layout.keyboard {
            ctx.set_font("sans-serif", 14.0);
            ctx.set_fill_style(Color::rgba(255, 255, 255, 170));
            let top = main_bottom(&layout.widgets) + MARGIN * 1.5;
            for (index, line) in CONTROLS.iter().enumerate() {
                ctx.fill_text(line, center, top + (index as f32 + 0.5) * LINE_HEIGHT);
            }
        }
    }

    fn draw_field(&self, ctx: &mut CanvasRenderingContext2d<'_>, page: Page) {
        let center = CONTENT_WIDTH / 2.0;
        let (label, help) = match page {
            Page::DirectConnect => (
                "Server Address",
                "Type an address like 192.168.1.5 and press Enter",
            ),
            Page::Username => ("Username", "Other players see this name above your head"),
            _ => ("World Name", "Type a name and press Enter"),
        };
        ctx.fill_text(label, center, FIELD_TOP - 20.0);
        ctx.set_fill_style(Color::rgb(160, 160, 160));
        ctx.fill_rect(WIDE_X, FIELD_TOP, WIDE_WIDTH, BUTTON_HEIGHT);
        ctx.set_fill_style(Color::rgb(0, 0, 0));
        ctx.fill_rect(
            WIDE_X + 2.0,
            FIELD_TOP + 2.0,
            WIDE_WIDTH - 4.0,
            BUTTON_HEIGHT - 4.0,
        );
        ctx.set_text_align(TextAlign::Left);
        ctx.set_fill_style(Color::rgb(255, 255, 255));
        ctx.fill_text(
            format!("{}_", self.name),
            WIDE_X + 12.0,
            FIELD_TOP + BUTTON_HEIGHT / 2.0,
        );
        ctx.set_text_align(TextAlign::Center);
        ctx.set_font("sans-serif", 13.0);
        if let Some(error) = &self.error {
            ctx.set_fill_style(Color::rgb(255, 110, 110));
            ctx.fill_text(error, center, FIELD_TOP + 64.0);
        } else {
            ctx.set_fill_style(Color::rgba(255, 255, 255, 170));
            ctx.fill_text(help, center, FIELD_TOP + 64.0);
        }
        ctx.set_font("sans-serif", 15.0);
    }

    fn draw_list(
        &self,
        ctx: &mut CanvasRenderingContext2d<'_>,
        hovered: Option<Action>,
        rows: &[(String, String)],
    ) {
        ctx.set_fill_style(Color::rgba(0, 0, 0, 120));
        ctx.fill_rect(
            WIDE_X,
            LIST_TOP - 4.0,
            WIDE_WIDTH,
            LIST_BOTTOM - LIST_TOP + 4.0,
        );
        ctx.set_text_align(TextAlign::Left);
        for (row, (title, details)) in rows.iter().enumerate().skip(self.scroll).take(LIST_ROWS) {
            let y = LIST_TOP + (row - self.scroll) as f32 * ROW_HEIGHT;
            if self.selected == Some(row) {
                ctx.set_fill_style(Color::rgb(200, 200, 200));
                ctx.fill_rect(WIDE_X, y, WIDE_WIDTH, ROW_HEIGHT - 4.0);
                ctx.set_fill_style(Color::rgb(0, 0, 0));
                ctx.fill_rect(WIDE_X + 2.0, y + 2.0, WIDE_WIDTH - 4.0, ROW_HEIGHT - 8.0);
            } else if hovered == Some(Action::Row(row)) {
                ctx.set_fill_style(Color::rgba(255, 255, 255, 30));
                ctx.fill_rect(WIDE_X, y, WIDE_WIDTH, ROW_HEIGHT - 4.0);
            }
            ctx.set_font_weight(FontWeight::Bold);
            ctx.set_font("sans-serif", 15.0);
            ctx.set_fill_style(Color::rgb(255, 255, 255));
            ctx.fill_text(title, WIDE_X + 12.0, y + 14.0);
            ctx.set_font_weight(FontWeight::Normal);
            ctx.set_font("sans-serif", 13.0);
            ctx.set_fill_style(Color::rgba(255, 255, 255, 150));
            ctx.fill_text(details, WIDE_X + 12.0, y + 31.0);
        }
        if rows.len() > LIST_ROWS {
            let height = LIST_BOTTOM - LIST_TOP;
            let thumb = height * LIST_ROWS as f32 / rows.len() as f32;
            let top = (height - thumb) * self.scroll as f32 / self.max_scroll() as f32;
            ctx.set_fill_style(Color::rgba(255, 255, 255, 120));
            ctx.fill_rect(WIDE_X + WIDE_WIDTH - 6.0, LIST_TOP + top, 4.0, thumb - 4.0);
        }
        ctx.set_text_align(TextAlign::Center);
        ctx.set_font("sans-serif", 15.0);
    }
}

// The bottom of the buttons of the title and pause pages.
fn main_bottom(widgets: &[Widget]) -> f32 {
    widgets
        .last()
        .map_or(0.0, |widget| widget.y + BUTTON_HEIGHT)
}

// When a world was last played, in words.
fn played(modified: SystemTime) -> String {
    let seconds = modified.elapsed().map_or(0, |elapsed| elapsed.as_secs());
    let (value, unit) = match seconds {
        0..60 => return "Played just now".to_owned(),
        60..3600 => (seconds / 60, "minute"),
        3600..86_400 => (seconds / 3600, "hour"),
        _ => (seconds / 86_400, "day"),
    };
    let plural = if value == 1 { "" } else { "s" };
    format!("Played {value} {unit}{plural} ago")
}

fn draw_slider(
    ctx: &mut CanvasRenderingContext2d<'_>,
    widget: &Widget,
    hovered: bool,
    settings: Settings,
) {
    let (label, fraction, help) = if widget.action == Action::Distance {
        (
            format!("Render Distance: {} chunks", settings.distance),
            crate::settings::distance_fraction(settings.distance),
            "Lower uses less memory and loads fewer chunks",
        )
    } else {
        (
            format!("Sound Volume: {}%", settings.volume),
            f32::from(settings.volume) / f32::from(crate::settings::MAX_VOLUME),
            "Blocks, footsteps, jumps, flying, water and menus",
        )
    };
    let (center, y) = (CONTENT_WIDTH / 2.0, widget.y);
    ctx.set_fill_style(Color::rgb(255, 255, 255));
    ctx.fill_text(&label, center, y + 4.0);
    ctx.set_fill_style(Color::rgb(45, 45, 45));
    ctx.fill_rect(widget.x, y + 22.0, widget.width, 8.0);
    ctx.set_fill_style(if hovered {
        Color::rgb(255, 255, 160)
    } else {
        Color::rgb(160, 170, 220)
    });
    ctx.fill_rect(widget.x, y + 22.0, fraction * widget.width, 8.0);
    ctx.fill_rect(
        widget.x + fraction * (widget.width - 12.0),
        y + 17.0,
        12.0,
        18.0,
    );
    ctx.set_font("sans-serif", 13.0);
    ctx.set_fill_style(Color::rgba(255, 255, 255, 170));
    ctx.fill_text(help, center, y + 53.0);
    ctx.set_font("sans-serif", 15.0);
}

fn draw_button(
    ctx: &mut CanvasRenderingContext2d<'_>,
    widget: &Widget,
    label: &str,
    hovered: bool,
) {
    let (x, y, width, height) = (widget.x, widget.y, widget.width, BUTTON_HEIGHT);
    ctx.set_fill_style(Color::rgb(0, 0, 0));
    ctx.fill_rect(x, y, width, height);
    ctx.set_fill_style(if hovered {
        Color::rgb(126, 136, 191)
    } else if widget.enabled {
        Color::rgb(111, 111, 111)
    } else {
        Color::rgb(60, 60, 60)
    });
    ctx.fill_rect(x + 2.0, y + 2.0, width - 4.0, height - 4.0);
    ctx.set_fill_style(Color::rgba(255, 255, 255, 90));
    ctx.fill_rect(x + 2.0, y + 2.0, width - 4.0, 2.0);
    ctx.fill_rect(x + 2.0, y + 2.0, 2.0, height - 4.0);
    ctx.set_fill_style(Color::rgba(0, 0, 0, 90));
    ctx.fill_rect(x + 2.0, y + height - 4.0, width - 4.0, 2.0);
    ctx.fill_rect(x + width - 4.0, y + 2.0, 2.0, height - 4.0);
    let (center, label_y) = (x + width / 2.0, y + height / 2.0);
    ctx.set_fill_style(Color::rgba(0, 0, 0, 150));
    ctx.fill_text(label, center + 1.0, label_y + 1.0);
    ctx.set_fill_style(if hovered {
        Color::rgb(255, 255, 160)
    } else if widget.enabled {
        Color::rgb(255, 255, 255)
    } else {
        Color::rgb(160, 160, 160)
    });
    ctx.fill_text(label, center, label_y);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu(page: Page, worlds: usize) -> Menu {
        let mut menu = Menu::default();
        menu.show_worlds(
            (0..worlds)
                .map(|index| World {
                    name: format!("World {index}"),
                    modified: SystemTime::now(),
                })
                .collect(),
            None,
        );
        menu.page = page;
        menu
    }

    #[test]
    fn widgets_and_sliders_follow_scaled_safe_area_layout() {
        for size in [
            LogicalSize::new(1000.0, 600.0),
            LogicalSize::new(320.0, 480.0),
            LogicalSize::new(700.0, 300.0),
        ] {
            for page in [
                Page::Title,
                Page::Pause,
                Page::Worlds,
                Page::Create,
                Page::Rename,
                Page::Delete,
                Page::Multiplayer,
                Page::DirectConnect,
                Page::Username,
                Page::Settings,
            ] {
                for keyboard in [false, true] {
                    let mut menu = menu(page, 8);
                    menu.session = Session::Host { lan: None };
                    let layout = Layout::new(size, LogicalInsets::default(), keyboard, &menu);
                    assert!(layout.content.1 + layout.height * layout.scale <= layout.footer_y);
                    for widget in &layout.widgets {
                        let point = LogicalPoint::new(
                            layout.content.0 + (widget.x + widget.width / 2.0) * layout.scale,
                            layout.content.1 + (widget.y + 20.0) * layout.scale,
                        );
                        assert_eq!(
                            layout.action_at(point),
                            widget.enabled.then_some(widget.action)
                        );
                    }
                    let point = LogicalPoint::new(layout.content.0 - 100.0, 0.0);
                    assert_eq!(layout.slider_fraction(point), 0.0);
                    assert_eq!(
                        layout.slider_fraction(LogicalPoint::new(size.width + 100.0, 0.0)),
                        1.0
                    );
                    let point = LogicalPoint::new(
                        layout.content.0 + (WIDE_X + WIDE_WIDTH * 0.75) * layout.scale,
                        0.0,
                    );
                    assert!((layout.slider_fraction(point) - 0.75).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn world_buttons_need_a_selected_world_and_rows_scroll() {
        let empty = menu(Page::Worlds, 0);
        assert_eq!(empty.selected, None);
        let layout = Layout::new(
            LogicalSize::new(1000.0, 800.0),
            LogicalInsets::default(),
            true,
            &empty,
        );
        let enabled: Vec<_> = layout
            .widgets
            .iter()
            .filter(|widget| widget.enabled)
            .map(|widget| widget.action)
            .collect();
        assert_eq!(enabled, [Action::CreateWorld, Action::Back]);

        let mut menu = menu(Page::Worlds, 8);
        assert_eq!(menu.selected_world(), Some("World 0"));
        let rows = |menu: &Menu| {
            Layout::widgets(Page::Worlds, true, menu)
                .1
                .iter()
                .filter_map(|widget| match widget.action {
                    Action::Row(index) => Some(index),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(rows(&menu), [0, 1, 2, 3, 4]);
        menu.scroll_steps(-10);
        assert_eq!(rows(&menu), [3, 4, 5, 6, 7]);
        menu.scroll_steps(1);
        assert_eq!(rows(&menu), [2, 3, 4, 5, 6]);
        menu.select_step(-1);
        assert_eq!(menu.selected, Some(0));
        assert_eq!(rows(&menu), [0, 1, 2, 3, 4]);
        menu.select_step(10);
        assert_eq!(menu.selected_world(), Some("World 7"));
        assert_eq!(rows(&menu), [3, 4, 5, 6, 7]);
        let worlds = std::mem::take(&mut menu.worlds);
        menu.show_worlds(worlds, Some("World 6"));
        assert_eq!(menu.selected, Some(6));
        assert_eq!(rows(&menu), [2, 3, 4, 5, 6]);
    }

    #[test]
    fn pause_buttons_follow_the_session() {
        let mut menu = menu(Page::Pause, 0);
        let actions = |menu: &Menu| {
            Layout::widgets(Page::Pause, true, menu)
                .1
                .iter()
                .map(|widget| (widget.action, widget.enabled))
                .collect::<Vec<_>>()
        };
        menu.session = Session::Guest;
        assert_eq!(
            menu.label(Page::Pause, Action::QuitToTitle, Settings::default()),
            "Disconnect"
        );
        assert!(!actions(&menu).contains(&(Action::OpenToLan, true)));
        menu.session = Session::Host { lan: None };
        assert!(actions(&menu).contains(&(Action::OpenToLan, true)));
        menu.session = Session::Host { lan: Some(47820) };
        assert!(actions(&menu).contains(&(Action::OpenToLan, false)));
        assert_eq!(
            menu.label(Page::Pause, Action::OpenToLan, Settings::default()),
            "LAN Open on Port 47820"
        );
    }

    #[test]
    fn found_lan_worlds_keep_their_selection() {
        let found = |host: &str, port: u16| Found {
            address: std::net::SocketAddr::from(([192, 168, 1, 5], port)),
            announcement: crate::protocol::Announcement {
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
        let layout = Layout::widgets(Page::Multiplayer, true, &menu).1;
        assert!(
            layout
                .iter()
                .any(|widget| widget.action == Action::JoinServer && !widget.enabled)
        );
        assert_eq!(menu.rows()[0].1, "Alice - 2 players - 192.168.1.5:47820");
    }

    #[test]
    fn last_played_times_read_naturally() {
        let ago = |seconds| played(SystemTime::now() - std::time::Duration::from_secs(seconds));
        assert_eq!(ago(5), "Played just now");
        assert_eq!(ago(60), "Played 1 minute ago");
        assert_eq!(ago(7200), "Played 2 hours ago");
        assert_eq!(ago(86_400 * 3), "Played 3 days ago");
    }
}
