/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The title, pause, world, multiplayer, settings and build tools pages that every platform
//! shows over the dimmed world: the pages, their buttons, lists, text fields and sliders, the
//! layout that fits them on the screen, and drawing them on a platform's [`Canvas`]. The
//! platforms handle the chosen [`Action`]s.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::{format, vec};
use core::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

#[cfg(not(feature = "std"))]
use crate::math::Float;

/// The game's name, the title of the title page.
pub const TITLE: &str = "PlaatCraft";
const DESCRIPTION: &str = "A Minecraft like voxel sandbox game";
/// The line at the bottom of the menus.
pub const FOOTER: &str = "Made by Bastiaan van der Plaat";
/// The width of the pages' content.
pub const CONTENT_WIDTH: f32 = 440.0;
/// The height of the footer line.
pub const FOOTER_HEIGHT: f32 = 24.0;
const BUTTON_WIDTH: f32 = 200.0;
const BUTTON_HEIGHT: f32 = 40.0;
const WIDE_WIDTH: f32 = 360.0;
const WIDE_X: f32 = (CONTENT_WIDTH - WIDE_WIDTH) / 2.0;
const HALF_WIDTH: f32 = 175.0;
// The tagline ends near 79, so the first button gets the same gap as the controls below it.
const HEADER_HEIGHT: f32 = 106.0;
const MARGIN: f32 = 16.0;
const LINE_HEIGHT: f32 = 20.0;
const LIST_TOP: f32 = 80.0;
const LIST_ROWS: usize = 5;
const ROW_HEIGHT: f32 = 48.0;
const LIST_BOTTOM: f32 = LIST_TOP + LIST_ROWS as f32 * ROW_HEIGHT;
const FIELD_TOP: f32 = 110.0;
// The QWERTY on-screen keyboard of consoles below the text field, 31 half keys wide like a
// keyboard. The rows of character keys without and with shift, and where they start in half
// keys after Tab, Caps Lock and Shift.
const KEY_SIZE: f32 = 28.0;
const KEYS_TOP: f32 = FIELD_TOP + 90.0;
const KEYS: [&[u8]; 4] = [
    b"`1234567890-=",
    b"qwertyuiop[]\\",
    b"asdfghjkl;'",
    b"zxcvbnm,./",
];
const SHIFTED_KEYS: [&[u8]; 4] = [
    b"~!@#$%^&*()_+",
    b"QWERTYUIOP{}|",
    b"ASDFGHJKL:\"",
    b"ZXCVBNM<>?",
];
const KEY_STARTS: [usize; 4] = [0, 4, 4, 5];
const KEY_HALVES: usize = 31;
/// The build tools and the commands they run, the last one opens the chat.
pub const TOOLS: [(&str, &str); 8] = [
    ("Mark First Corner", "/pos1"),
    ("Mark Second Corner", "/pos2"),
    ("Fill With Held Block", "/fill"),
    ("Clear Selection", "/clear"),
    ("Undo", "/undo"),
    ("Toggle Flying", "/fly"),
    ("Toggle Day and Night", "/time"),
    ("Chat and Commands", ""),
];
/// The fewest chunks the render distance can be.
pub const MIN_DISTANCE: u8 = 4;
/// The most chunks the render distance can be.
pub const MAX_DISTANCE: u8 = 128;
/// The loudest sound volume, in percent.
pub const MAX_VOLUME: u8 = 100;

/// A color as red, green, blue and alpha.
pub type Color = [u8; 4];
const WHITE: Color = [255, 255, 255, 255];
const FADED: Color = [255, 255, 255, 170];

/// Where text lies from its x position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// The text starts at x.
    Left,
    /// The text is centered on x.
    Center,
}

/// How text is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font {
    /// The height in pixels.
    pub size: f32,
    /// Whether the text is bold.
    pub bold: bool,
    /// Where the text lies from its x position.
    pub align: Align,
    /// The color of the text.
    pub color: Color,
}

/// Where a platform draws the menus.
pub trait Canvas {
    /// Fills a rectangle.
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color);
    /// Draws a line of text with its middle at y.
    fn text(&mut self, text: &str, x: f32, y: f32, font: Font);
}

/// What the platform's players type and play with, which changes the pages.
#[derive(Clone, Copy, Debug)]
pub struct Platform {
    /// How the player types.
    pub keyboard: Keyboard,
    /// Whether the game can quit itself, iOS apps can't.
    pub quits: bool,
    /// Whether the render distance can be changed.
    pub distance: bool,
    /// The key that finishes typing, like Enter.
    pub confirm: &'static str,
    /// How a world of a list is opened, like "Click a world twice".
    pub open: &'static str,
    /// The controls shown on the title and pause pages.
    pub controls: &'static [&'static str],
}

/// How the player types text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {
    /// A hardware keyboard.
    Physical,
    /// A keyboard of buttons on the page, for consoles.
    OnScreen,
    /// No typing, like touch screens, the pages that need typing are left out.
    None,
}

/// The settings the settings page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Settings {
    /// The render distance in chunks.
    pub distance: u8,
    /// Whether the debug info shows.
    pub show_stats: bool,
    /// The sound volume in percent.
    pub volume: u8,
    /// Whether walking into a block steps on it.
    pub auto_jump: bool,
}

fn distance_exponent() -> f32 {
    // Reserve the last quarter of the slider for half the maximum distance.
    let midpoint = f32::from(MAX_DISTANCE / 2 - MIN_DISTANCE);
    (midpoint / f32::from(MAX_DISTANCE - MIN_DISTANCE)).ln() / 0.75_f32.ln()
}

/// The render distance at a fraction of the slider.
pub fn distance_at(fraction: f32) -> u8 {
    MIN_DISTANCE
        + (fraction.clamp(0.0, 1.0).powf(distance_exponent())
            * f32::from(MAX_DISTANCE - MIN_DISTANCE))
        .round() as u8
}

/// Where a render distance lies along its slider.
pub fn distance_fraction(distance: u8) -> f32 {
    (f32::from(distance.clamp(MIN_DISTANCE, MAX_DISTANCE) - MIN_DISTANCE)
        / f32::from(MAX_DISTANCE - MIN_DISTANCE))
    .powf(1.0 / distance_exponent())
}

/// When a world was last played, in words.
pub fn played(seconds_ago: u64) -> String {
    let (value, unit) = match seconds_ago {
        0..60 => return "Played just now".into(),
        60..3600 => (seconds_ago / 60, "minute"),
        3600..86_400 => (seconds_ago / 3600, "hour"),
        _ => (seconds_ago / 86_400, "day"),
    };
    let plural = if value == 1 { "" } else { "s" };
    format!("Played {value} {unit}{plural} ago")
}

/// A page of the menus.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Page {
    /// The title page with the turning world behind it.
    #[default]
    Title,
    /// The pause page of a played world.
    Pause,
    /// The saved worlds.
    Worlds,
    /// Naming a new world.
    Create,
    /// Renaming the selected world.
    Rename,
    /// Confirming to delete the selected world.
    Delete,
    /// The worlds on the LAN.
    Multiplayer,
    /// Typing the address of a world.
    DirectConnect,
    /// Typing the player name.
    Username,
    /// The settings.
    Settings,
    /// The build tools, for platforms without a chat to type commands in.
    Tools,
    /// Typing a chat message or command, for platforms without a chat overlay.
    Chat,
}

impl Page {
    /// Whether the page has a text field.
    pub const fn typing(self) -> bool {
        matches!(
            self,
            Self::Create | Self::Rename | Self::DirectConnect | Self::Username | Self::Chat
        )
    }

    /// Whether the page has a list of worlds.
    pub const fn listing(self) -> bool {
        matches!(self, Self::Worlds | Self::Multiplayer)
    }

    const fn main(self) -> bool {
        matches!(self, Self::Title | Self::Pause)
    }
}

/// What a button, list row, slider or key of a page does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs, reason = "The labels say what they do")]
pub enum Action {
    SinglePlayer,
    MultiPlayer,
    Resume,
    Settings,
    Quit,
    OpenToLan,
    QuitToTitle,
    /// A row of the shown list.
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
    /// A build tool of [`TOOLS`].
    Tool(usize),
    /// A character key of the on-screen keyboard, counted through its rows.
    Key(usize),
    Space,
    Erase,
    /// Types a tab, which text draws four spaces wide.
    Tab,
    /// Shifts the next key of the on-screen keyboard, `true` for the right shift key.
    Shift(bool),
    /// Shifts the letters of the on-screen keyboard until pressed again.
    CapsLock,
    /// The enter key of the on-screen keyboard, which confirms the text.
    Enter,
    Back,
}

/// How the player is in a world, which changes the pause page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Session {
    /// No world is played.
    #[default]
    None,
    /// The player hosts the world, maybe opened to the LAN on a port.
    Host {
        /// The port of the opened LAN.
        lan: Option<u16>,
    },
    /// The player plays a world of another player.
    Guest,
}

/// A row of a list: a world or a LAN world.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Row {
    /// The name of the world.
    pub title: String,
    /// The line below it.
    pub details: String,
    /// Whether it can be played.
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug)]
struct Widget {
    action: Action,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    enabled: bool,
}

impl Widget {
    const fn new(action: Action, x: f32, y: f32, width: f32) -> Self {
        Self {
            action,
            x,
            y,
            width,
            height: BUTTON_HEIGHT,
            enabled: true,
        }
    }

    const fn button(action: Action, y: f32) -> Self {
        Self::new(
            action,
            (CONTENT_WIDTH - BUTTON_WIDTH) / 2.0,
            y,
            BUTTON_WIDTH,
        )
    }

    const fn wide(action: Action, y: f32) -> Self {
        Self::new(action, WIDE_X, y, WIDE_WIDTH)
    }

    // A button in the left or right half of a row.
    const fn half(action: Action, right: bool, y: f32, enabled: bool) -> Self {
        let x = if right {
            CONTENT_WIDTH - WIDE_X - HALF_WIDTH
        } else {
            WIDE_X
        };
        Self {
            enabled,
            ..Self::new(action, x, y, HALF_WIDTH)
        }
    }

    fn contains(&self, [x, y]: [f32; 2]) -> bool {
        (self.x..self.x + self.width).contains(&x) && (self.y..self.y + self.height).contains(&y)
    }

    fn center(&self) -> [f32; 2] {
        [self.x + self.width / 2.0, self.y + self.height / 2.0]
    }
}

/// Where the widgets of the shown page are on the screen.
pub struct Layout {
    /// The top left of the content.
    pub content: [f32; 2],
    /// The height of the content.
    pub height: f32,
    /// The top of the footer.
    pub footer_y: f32,
    /// The scale of the content, which shrinks it to fit small screens.
    pub scale: f32,
    page: Page,
    platform: Platform,
    widgets: Vec<Widget>,
}

impl Layout {
    /// Lays out the shown page on a screen with its safe area insets: top, right, bottom and
    /// left.
    pub fn new(menu: &Menu, platform: Platform, size: [f32; 2], safe: [f32; 4]) -> Self {
        let [top, right, bottom, left] = safe;
        let (height, widgets) = Self::widgets(menu, platform);
        let width = (size[0] - left - right - 24.0).max(1.0);
        let available = (size[1] - top - bottom - 60.0).max(1.0);
        let scale = (width / CONTENT_WIDTH).min(available / height).min(1.0);
        Self {
            content: [
                left + (width + 24.0 - CONTENT_WIDTH * scale) / 2.0,
                top + (available - height * scale) / 2.0,
            ],
            height,
            footer_y: size[1] - bottom - 32.0,
            scale,
            page: menu.page,
            platform,
            widgets,
        }
    }

    // The content height and the widgets of a page.
    fn widgets(menu: &Menu, platform: Platform) -> (f32, Vec<Widget>) {
        let typing = platform.keyboard != Keyboard::None;
        let main = |actions: &[(Action, bool)]| {
            let widgets: Vec<_> = actions
                .iter()
                .enumerate()
                .map(|(index, &(action, enabled))| Widget {
                    enabled,
                    ..Widget::button(action, HEADER_HEIGHT + index as f32 * 50.0)
                })
                .collect();
            let controls = if platform.controls.is_empty() {
                0.0
            } else {
                MARGIN * 1.5 + platform.controls.len() as f32 * LINE_HEIGHT
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
            (menu.scroll..menu.rows.len())
                .take(LIST_ROWS)
                .enumerate()
                .map(|(row, index)| Widget {
                    height: ROW_HEIGHT - 4.0,
                    ..Widget::wide(Action::Row(index), LIST_TOP + row as f32 * ROW_HEIGHT)
                })
                .collect()
        };
        let y = LIST_BOTTOM + 16.0;
        match menu.page {
            Page::Title => {
                let mut actions = vec![
                    (Action::SinglePlayer, true),
                    (Action::MultiPlayer, true),
                    (Action::Settings, true),
                ];
                if platform.quits {
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
                // Without a keyboard there is no typing a new name.
                if typing {
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
                let joinable = menu.selected_row().is_some_and(|row| row.enabled);
                let mut widgets = rows(menu);
                widgets.push(Widget::half(Action::JoinServer, false, y, joinable));
                // Without a keyboard there is no typing an address.
                if typing {
                    widgets.push(Widget::half(Action::DirectConnect, true, y, true));
                    widgets.push(Widget::button(Action::Back, y + 50.0));
                    (y + 90.0, widgets)
                } else {
                    widgets.push(Widget::half(Action::Back, true, y, true));
                    (y + 40.0, widgets)
                }
            }
            page if page.typing() && platform.keyboard == Keyboard::OnScreen => {
                let half = KEY_SIZE / 2.0;
                let left = (CONTENT_WIDTH - KEY_HALVES as f32 * half) / 2.0;
                let key = |action, row: usize, start: usize, halves: usize| Widget {
                    height: KEY_SIZE - 2.0,
                    ..Widget::new(
                        action,
                        left + start as f32 * half,
                        KEYS_TOP + row as f32 * KEY_SIZE,
                        halves as f32 * half - 2.0,
                    )
                };
                let mut widgets = Vec::new();
                let mut index = 0;
                for (row, keys) in KEYS.iter().enumerate() {
                    for column in 0..keys.len() {
                        // The backslash ends its row a bit wider, like on keyboards
                        let halves = if row == 1 && column == keys.len() - 1 {
                            3
                        } else {
                            2
                        };
                        let start = KEY_STARTS[row] + column * 2;
                        widgets.push(key(Action::Key(index), row, start, halves));
                        index += 1;
                    }
                }
                widgets.push(key(Action::Erase, 0, 26, 5));
                widgets.push(key(Action::Tab, 1, 0, 4));
                widgets.push(key(Action::CapsLock, 2, 0, 4));
                widgets.push(key(Action::Enter, 2, 26, 5));
                widgets.push(key(Action::Shift(false), 3, 0, 5));
                widgets.push(key(Action::Shift(true), 3, 25, 6));
                widgets.push(key(Action::Space, 4, 7, 17));
                let y = KEYS_TOP + 5.0 * KEY_SIZE + 10.0;
                widgets.push(Widget::button(Action::Back, y));
                (y + BUTTON_HEIGHT, widgets)
            }
            page if page.typing() => (FIELD_TOP + 140.0, confirm(FIELD_TOP + 100.0)),
            Page::Delete => (190.0, confirm(150.0)),
            Page::Tools => {
                let mut widgets: Vec<_> = (0..TOOLS.len())
                    .map(|index| Widget::wide(Action::Tool(index), LIST_TOP + index as f32 * 46.0))
                    .collect();
                let y = LIST_TOP + TOOLS.len() as f32 * 46.0;
                widgets.push(Widget::button(Action::Back, y));
                (y + BUTTON_HEIGHT, widgets)
            }
            _ => {
                let mut widgets = Vec::new();
                let mut y = 80.0;
                if platform.distance {
                    widgets.push(Widget::wide(Action::Distance, y));
                    y += 80.0;
                }
                widgets.push(Widget::wide(Action::Volume, y));
                widgets.push(Widget::button(Action::Stats, y + 80.0));
                widgets.push(Widget::button(Action::AutoJump, y + 130.0));
                if typing {
                    widgets.push(Widget::button(Action::Username, y + 180.0));
                }
                let y = main_bottom(&widgets) + 10.0;
                widgets.push(Widget::button(Action::Reset, y));
                widgets.push(Widget::button(Action::Back, y + 50.0));
                (y + 100.0, widgets)
            }
        }
    }

    /// A screen position in content coordinates.
    pub fn local(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [
            (x - self.content[0]) / self.scale,
            (y - self.content[1]) / self.scale,
        ]
    }

    /// The enabled action at a screen position.
    pub fn action_at(&self, position: [f32; 2]) -> Option<Action> {
        let point = self.local(position);
        self.widgets
            .iter()
            .find(|widget| widget.enabled && widget.contains(point))
            .map(|widget| widget.action)
    }

    /// Whether a screen position is over the list.
    pub fn in_list(&self, position: [f32; 2]) -> bool {
        let [x, y] = self.local(position);
        self.page.listing()
            && (WIDE_X..WIDE_X + WIDE_WIDTH).contains(&x)
            && (LIST_TOP..LIST_BOTTOM).contains(&y)
    }

    /// The position along a slider, from 0.0 to 1.0.
    pub fn slider_fraction(&self, position: [f32; 2]) -> f32 {
        ((self.local(position)[0] - WIDE_X) / WIDE_WIDTH).clamp(0.0, 1.0)
    }

    /// The actions of the page in order, the enabled ones.
    pub fn actions(&self) -> impl Iterator<Item = Action> + '_ {
        self.widgets
            .iter()
            .filter(|widget| widget.enabled)
            .map(|widget| widget.action)
    }
}

/// The state of the menus.
#[derive(Default)]
pub struct Menu {
    /// The shown page.
    pub page: Page,
    /// How the player is in a world.
    pub session: Session,
    /// The player name.
    pub username: String,
    /// The rows of the shown list.
    pub rows: Vec<Row>,
    /// The selected row of the shown list.
    pub selected: Option<usize>,
    scroll: usize,
    // Where dragging the list started, and its scroll then.
    list_drag: Option<(f32, usize)>,
    /// The typed text.
    pub text: String,
    /// Why the typed text or the list can't be used.
    pub error: Option<String>,
    // The widget the D-pad or pointer is on, for consoles.
    focus: Option<Action>,
    // Whether the on-screen keyboard shifts the next key, and its caps lock.
    shift: bool,
    caps_lock: bool,
}

// Everything that shows, so platforms can draw a page again only when it changed.
impl Hash for Menu {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.page, self.session, &self.username, &self.rows).hash(state);
        (self.selected, self.scroll, &self.text, &self.error).hash(state);
    }
}

impl Menu {
    /// Shows a page, a text page with the text.
    pub fn show(&mut self, page: Page, text: String) {
        self.page = page;
        self.text = text;
        self.error = None;
        self.focus = None;
        self.shift = false;
    }

    /// Shows a list page with its rows and the row selected, or else the first.
    pub fn show_list(&mut self, page: Page, rows: Vec<Row>, selected: Option<usize>) {
        self.set_rows(rows, selected);
        self.scroll = 0;
        self.scroll_to_selected();
        self.show(page, String::new());
    }

    /// Updates the rows of the shown list, like newly found LAN worlds, with a row selected.
    pub fn set_rows(&mut self, rows: Vec<Row>, selected: Option<usize>) {
        self.selected = selected
            .filter(|&selected| selected < rows.len())
            .or_else(|| (!rows.is_empty()).then_some(0));
        self.rows = rows;
        self.scroll = self.scroll.min(self.max_scroll());
    }

    /// The selected row of the shown list.
    pub fn selected_row(&self) -> Option<&Row> {
        self.rows.get(self.selected?)
    }

    const fn max_scroll(&self) -> usize {
        self.rows.len().saturating_sub(LIST_ROWS)
    }

    fn scroll_to_selected(&mut self) {
        if let Some(selected) = self.selected {
            self.scroll = self
                .scroll
                .clamp(selected.saturating_sub(LIST_ROWS - 1), selected);
        }
    }

    /// Scrolls the list, positive steps scroll up.
    pub fn scroll_steps(&mut self, steps: i64) {
        self.scroll = self
            .scroll
            .saturating_add_signed(-steps as isize)
            .min(self.max_scroll());
    }

    /// Moves the selection, like with the arrow keys.
    pub fn select_step(&mut self, step: isize) {
        let count = self.rows.len();
        if count > 0 {
            self.selected = Some(
                self.selected
                    .map_or(0, |selected| selected.saturating_add_signed(step))
                    .min(count - 1),
            );
            self.scroll_to_selected();
        }
    }

    /// Starts dragging the list at a screen position.
    pub fn start_list_drag(&mut self, layout: &Layout, position: [f32; 2]) {
        self.list_drag = Some((layout.local(position)[1], self.scroll));
    }

    /// Scrolls the dragged list with the pointer.
    pub fn list_drag(&mut self, layout: &Layout, position: [f32; 2]) {
        if let Some((start, scroll)) = self.list_drag {
            let rows = ((start - layout.local(position)[1]) / ROW_HEIGHT).round() as isize;
            self.scroll = scroll.saturating_add_signed(rows).min(self.max_scroll());
        }
    }

    /// Stops dragging the list.
    pub const fn end_list_drag(&mut self) {
        self.list_drag = None;
    }

    /// Types a key of the on-screen keyboard, the text holds at most `max` characters.
    pub fn type_key(&mut self, action: Action, max: usize) {
        let room = self.text.chars().count() < max;
        match action {
            Action::Key(index) if room => {
                self.text.push(self.key(index));
                self.shift = false;
            }
            Action::Space if room => self.text.push(' '),
            Action::Erase => _ = self.text.pop(),
            Action::Tab if room => self.text.push('\t'),
            Action::Shift(_) => self.shift = !self.shift,
            Action::CapsLock => self.caps_lock = !self.caps_lock,
            _ => {}
        }
        self.error = None;
    }

    // The character of a key, caps lock only shifts the letters.
    fn key(&self, mut index: usize) -> char {
        let row = KEYS
            .iter()
            .position(|keys| {
                let found = index < keys.len();
                if !found {
                    index -= keys.len();
                }
                found
            })
            .expect("Key of the keyboard");
        let key = KEYS[row][index];
        let shifted = self.shift != (self.caps_lock && key.is_ascii_alphabetic());
        char::from(if shifted { SHIFTED_KEYS } else { KEYS }[row][index])
    }

    /// The widget the D-pad or pointer is on, for consoles.
    pub fn focused(&self, layout: &Layout) -> Option<Action> {
        self.focus
            .filter(|&focus| layout.actions().any(|action| action == focus))
            .or_else(|| layout.actions().next())
    }

    /// Puts the focus on the widget at a screen position.
    pub fn point(&mut self, layout: &Layout, position: [f32; 2]) {
        if let Some(action) = layout.action_at(position) {
            self.focus = Some(action);
        }
    }

    /// Moves the focus to the nearest widget in a direction, with dx and dy from -1 to 1, the
    /// list scrolls and rows are selected on the way.
    pub fn step(&mut self, layout: &Layout, [dx, dy]: [f32; 2]) {
        let Some(current) = self.focused(layout) else {
            return;
        };
        if let Action::Row(index) = current {
            let next = index as isize + dy as isize;
            if dy != 0.0 && (0..self.rows.len() as isize).contains(&next) {
                let next = next as usize;
                self.selected = Some(next);
                self.focus = Some(Action::Row(next));
                if !(self.scroll..self.scroll + LIST_ROWS).contains(&next) {
                    self.scroll_to_selected();
                }
                return;
            }
        }
        let Some(widget) = layout
            .widgets
            .iter()
            .find(|widget| widget.action == current)
        else {
            return;
        };
        let [x, y] = widget.center();
        let best = layout
            .widgets
            .iter()
            .filter(|other| other.enabled && other.action != current)
            .filter_map(|other| {
                let [cx, cy] = other.center();
                let along = (cx - x) * dx + (cy - y) * dy;
                let across = ((cx - x) * dy).abs() + ((cy - y) * dx).abs();
                // Sideways moves stay in the row
                let same_row = dy != 0.0 || across < other.height;
                (along > 1.0 && same_row).then_some((other.action, along + across * 2.0))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((action, _)) = best {
            self.focus = Some(action);
            if let Action::Row(index) = action {
                self.selected = Some(index);
            }
        }
    }

    fn subtitle(&self, platform: Platform) -> String {
        let open = platform.open;
        match self.page {
            Page::Title | Page::Pause => DESCRIPTION.into(),
            page if page.listing()
                && let Some(error) = &self.error =>
            {
                error.clone()
            }
            Page::Worlds if self.rows.is_empty() => "Create a world to start playing".into(),
            Page::Worlds => format!("{open} to play it"),
            Page::Multiplayer => {
                let name = &self.username;
                if self.rows.is_empty() {
                    format!("Playing as {name}, searching the LAN for worlds...")
                } else {
                    let open = open.to_lowercase();
                    format!("Playing as {name}, {open} to join it")
                }
            }
            Page::Tools => "Mark two corners, then fill or clear the box between them".into(),
            _ => String::new(),
        }
    }

    /// The label of a button.
    pub fn label(&self, action: Action, settings: Settings) -> String {
        let page = self.page;
        let on = |name: &str, on: bool| format!("{name}: {}", if on { "ON" } else { "OFF" });
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
                Page::Chat => "Send",
                _ => "Delete",
            },
            Action::Back if page == Page::Settings => "Done",
            Action::Back if page.listing() || page == Page::Tools => "Back",
            Action::Back => "Cancel",
            Action::Stats => return on("Show Debug Info", settings.show_stats),
            Action::AutoJump => return on("Auto Jump", settings.auto_jump),
            Action::Username => return format!("Username: {}", self.username),
            Action::Reset => "Reset to Defaults",
            Action::Tool(index) => TOOLS[index].0,
            Action::Key(index) => return String::from(self.key(index)),
            Action::Space => "Space",
            Action::Erase => "Backspace",
            Action::Tab => "Tab",
            Action::Shift(_) if self.shift => "SHIFT",
            Action::Shift(_) => "Shift",
            Action::CapsLock if self.caps_lock => "CAPS",
            Action::CapsLock => "Caps",
            Action::Enter => "Enter",
            Action::Row(_) | Action::Distance | Action::Volume => "",
        };
        label.into()
    }

    const fn title(&self) -> &'static str {
        match self.page {
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
            Page::Tools => "Build Tools",
            Page::Chat => "Chat",
        }
    }

    /// Draws the page over the dimmed world and the footer, for platforms that draw every
    /// frame. The highlighted widget is under the pointer or has the focus.
    pub fn draw(
        &self,
        canvas: &mut impl Canvas,
        layout: &Layout,
        size: [f32; 2],
        highlighted: Option<Action>,
        settings: Settings,
    ) {
        canvas.rect(0.0, 0.0, size[0], size[1], [0, 0, 0, 170]);
        let mut content = Placed {
            canvas: &mut *canvas,
            origin: layout.content,
            scale: layout.scale,
        };
        self.draw_content(&mut content, layout, highlighted, settings);
        draw_footer(&mut Placed {
            canvas,
            origin: [layout.content[0], layout.footer_y],
            scale: layout.scale,
        });
    }

    /// Draws the page in content coordinates, see [`Layout`].
    pub fn draw_content(
        &self,
        canvas: &mut impl Canvas,
        layout: &Layout,
        highlighted: Option<Action>,
        settings: Settings,
    ) {
        let platform = layout.platform;
        let center = CONTENT_WIDTH / 2.0;
        let main = self.page.main();
        let size = if main { 48.0 } else { 32.0 };
        let title = self.title();
        canvas.text(
            title,
            center + 2.0,
            30.0,
            Font {
                size,
                bold: true,
                align: Align::Center,
                color: [0, 0, 0, 140],
            },
        );
        canvas.text(
            title,
            center,
            28.0,
            Font {
                size,
                bold: true,
                align: Align::Center,
                color: WHITE,
            },
        );
        let y = if main { 70.0 } else { 57.0 };
        canvas.text(
            &self.subtitle(platform),
            center,
            y,
            Font {
                size: 15.0,
                bold: false,
                align: Align::Center,
                color: WHITE,
            },
        );

        match self.page {
            page if page.listing() => self.draw_list(canvas, highlighted),
            page if page.typing() => self.draw_field(canvas, platform),
            Page::Delete => {
                let selected = self.selected_row().map_or("", |row| &row.title);
                let warning = format!("'{selected}' will be lost forever!");
                canvas.text(
                    &warning,
                    center,
                    90.0,
                    Font {
                        size: 15.0,
                        bold: false,
                        align: Align::Center,
                        color: WHITE,
                    },
                );
                let line = "This can't be undone";
                canvas.text(
                    line,
                    center,
                    115.0,
                    Font {
                        size: 15.0,
                        bold: false,
                        align: Align::Center,
                        color: FADED,
                    },
                );
            }
            _ => {}
        }

        for widget in &layout.widgets {
            let highlighted = highlighted == Some(widget.action);
            match widget.action {
                Action::Row(_) => {}
                Action::Distance | Action::Volume => {
                    draw_slider(canvas, widget, highlighted, settings);
                }
                action => draw_button(canvas, widget, &self.label(action, settings), highlighted),
            }
        }
        if main {
            let top = main_bottom(&layout.widgets) + MARGIN * 1.5;
            for (index, line) in platform.controls.iter().enumerate() {
                let y = top + (index as f32 + 0.5) * LINE_HEIGHT;
                canvas.text(
                    line,
                    center,
                    y,
                    Font {
                        size: 14.0,
                        bold: false,
                        align: Align::Center,
                        color: FADED,
                    },
                );
            }
        }
    }

    fn draw_field(&self, canvas: &mut impl Canvas, platform: Platform) {
        let center = CONTENT_WIDTH / 2.0;
        let (label, help) = match self.page {
            Page::DirectConnect => ("Server Address", "Type an address like 192.168.1.5"),
            Page::Username => ("Username", "Other players see this name above your head"),
            Page::Chat => ("Message", "Type a message or a command like /help"),
            _ => ("World Name", "Type a name"),
        };
        canvas.text(
            label,
            center,
            FIELD_TOP - 20.0,
            Font {
                size: 15.0,
                bold: false,
                align: Align::Center,
                color: WHITE,
            },
        );
        canvas.rect(
            WIDE_X,
            FIELD_TOP,
            WIDE_WIDTH,
            BUTTON_HEIGHT,
            [160, 160, 160, 255],
        );
        canvas.rect(
            WIDE_X + 2.0,
            FIELD_TOP + 2.0,
            WIDE_WIDTH - 4.0,
            BUTTON_HEIGHT - 4.0,
            [0, 0, 0, 255],
        );
        let text = format!("{}_", self.text);
        let y = FIELD_TOP + BUTTON_HEIGHT / 2.0;
        canvas.text(
            &text,
            WIDE_X + 12.0,
            y,
            Font {
                size: 15.0,
                bold: false,
                align: Align::Left,
                color: WHITE,
            },
        );
        let (line, color) = match &self.error {
            Some(error) => (error.clone(), [255, 110, 110, 255]),
            None => (format!("{help} and press {}", platform.confirm), FADED),
        };
        canvas.text(
            &line,
            center,
            FIELD_TOP + 64.0,
            Font {
                size: 13.0,
                bold: false,
                align: Align::Center,
                color,
            },
        );
    }

    fn draw_list(&self, canvas: &mut impl Canvas, highlighted: Option<Action>) {
        canvas.rect(
            WIDE_X,
            LIST_TOP - 4.0,
            WIDE_WIDTH,
            LIST_BOTTOM - LIST_TOP + 4.0,
            [0, 0, 0, 120],
        );
        for (row, Row { title, details, .. }) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(LIST_ROWS)
        {
            let y = LIST_TOP + (row - self.scroll) as f32 * ROW_HEIGHT;
            if self.selected == Some(row) {
                canvas.rect(
                    WIDE_X,
                    y,
                    WIDE_WIDTH,
                    ROW_HEIGHT - 4.0,
                    [200, 200, 200, 255],
                );
                canvas.rect(
                    WIDE_X + 2.0,
                    y + 2.0,
                    WIDE_WIDTH - 4.0,
                    ROW_HEIGHT - 8.0,
                    [0, 0, 0, 255],
                );
            } else if highlighted == Some(Action::Row(row)) {
                canvas.rect(WIDE_X, y, WIDE_WIDTH, ROW_HEIGHT - 4.0, [255, 255, 255, 30]);
            }
            let x = WIDE_X + 12.0;
            canvas.text(
                title,
                x,
                y + 14.0,
                Font {
                    size: 15.0,
                    bold: true,
                    align: Align::Left,
                    color: WHITE,
                },
            );
            let color = [255, 255, 255, 150];
            canvas.text(
                details,
                x,
                y + 31.0,
                Font {
                    size: 13.0,
                    bold: false,
                    align: Align::Left,
                    color,
                },
            );
        }
        if self.rows.len() > LIST_ROWS {
            let height = LIST_BOTTOM - LIST_TOP;
            let thumb = height * LIST_ROWS as f32 / self.rows.len() as f32;
            let top = (height - thumb) * self.scroll as f32 / self.max_scroll() as f32;
            let x = WIDE_X + WIDE_WIDTH - 6.0;
            canvas.rect(x, LIST_TOP + top, 4.0, thumb - 4.0, [255, 255, 255, 120]);
        }
    }
}

/// Draws the footer line in footer coordinates, [`CONTENT_WIDTH`] by [`FOOTER_HEIGHT`].
pub fn draw_footer(canvas: &mut impl Canvas) {
    let (x, y) = (CONTENT_WIDTH / 2.0, FOOTER_HEIGHT / 2.0);
    canvas.text(
        FOOTER,
        x,
        y,
        Font {
            size: 15.0,
            bold: false,
            align: Align::Center,
            color: FADED,
        },
    );
}

// A canvas moved and scaled to where the content is.
struct Placed<'a, C> {
    canvas: &'a mut C,
    origin: [f32; 2],
    scale: f32,
}

impl<C: Canvas> Canvas for Placed<'_, C> {
    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        let [left, top] = self.origin;
        let scale = self.scale;
        let (x, y) = (left + x * scale, top + y * scale);
        self.canvas.rect(x, y, width * scale, height * scale, color);
    }

    fn text(&mut self, text: &str, x: f32, y: f32, font: Font) {
        let [left, top] = self.origin;
        let scale = self.scale;
        let (x, y) = (left + x * scale, top + y * scale);
        let size = font.size * scale;
        self.canvas.text(text, x, y, Font { size, ..font });
    }
}

// The bottom of the buttons of the title and pause pages.
fn main_bottom(widgets: &[Widget]) -> f32 {
    widgets
        .last()
        .map_or(0.0, |widget| widget.y + BUTTON_HEIGHT)
}

fn draw_slider(canvas: &mut impl Canvas, widget: &Widget, highlighted: bool, settings: Settings) {
    let (label, fraction, help) = if widget.action == Action::Distance {
        (
            format!("Render Distance: {} chunks", settings.distance),
            distance_fraction(settings.distance),
            "Lower uses less memory and loads fewer chunks",
        )
    } else {
        (
            format!("Sound Volume: {}%", settings.volume),
            f32::from(settings.volume) / f32::from(MAX_VOLUME),
            "Blocks, footsteps, jumps, flying, water and menus",
        )
    };
    let (center, y) = (CONTENT_WIDTH / 2.0, widget.y);
    canvas.text(
        &label,
        center,
        y + 4.0,
        Font {
            size: 15.0,
            bold: false,
            align: Align::Center,
            color: WHITE,
        },
    );
    canvas.rect(widget.x, y + 22.0, widget.width, 8.0, [45, 45, 45, 255]);
    let color = if highlighted {
        [255, 255, 160, 255]
    } else {
        [160, 170, 220, 255]
    };
    canvas.rect(widget.x, y + 22.0, fraction * widget.width, 8.0, color);
    let thumb = widget.x + fraction * (widget.width - 12.0);
    canvas.rect(thumb, y + 17.0, 12.0, 18.0, color);
    canvas.text(
        help,
        center,
        y + 53.0,
        Font {
            size: 13.0,
            bold: false,
            align: Align::Center,
            color: FADED,
        },
    );
}

fn draw_button(canvas: &mut impl Canvas, widget: &Widget, label: &str, highlighted: bool) {
    let (x, y, width, height) = (widget.x, widget.y, widget.width, widget.height);
    canvas.rect(x, y, width, height, [0, 0, 0, 255]);
    let fill = if highlighted {
        [126, 136, 191, 255]
    } else if widget.enabled {
        [111, 111, 111, 255]
    } else {
        [60, 60, 60, 255]
    };
    canvas.rect(x + 2.0, y + 2.0, width - 4.0, height - 4.0, fill);
    canvas.rect(x + 2.0, y + 2.0, width - 4.0, 2.0, [255, 255, 255, 90]);
    canvas.rect(x + 2.0, y + 2.0, 2.0, height - 4.0, [255, 255, 255, 90]);
    canvas.rect(x + 2.0, y + height - 4.0, width - 4.0, 2.0, [0, 0, 0, 90]);
    canvas.rect(x + width - 4.0, y + 2.0, 2.0, height - 4.0, [0, 0, 0, 90]);
    let (center, label_y) = (x + width / 2.0, y + height / 2.0);
    let size = match widget.action {
        Action::Key(_) => 17.0,
        // The keyboard's wider keys have small labels like on keyboards
        Action::Erase | Action::Tab | Action::Shift(_) | Action::CapsLock | Action::Enter => 13.0,
        _ => 15.0,
    };
    canvas.text(
        label,
        center + 1.0,
        label_y + 1.0,
        Font {
            size,
            bold: false,
            align: Align::Center,
            color: [0, 0, 0, 150],
        },
    );
    let color = if highlighted {
        [255, 255, 160, 255]
    } else if widget.enabled {
        WHITE
    } else {
        [160, 160, 160, 255]
    };
    canvas.text(
        label,
        center,
        label_y,
        Font {
            size,
            bold: false,
            align: Align::Center,
            color,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: Platform = Platform {
        keyboard: Keyboard::Physical,
        quits: true,
        distance: true,
        confirm: "Enter",
        open: "Click a world twice",
        controls: &["Press Esc to pause"],
    };

    fn menu(page: Page, worlds: usize) -> Menu {
        let mut menu = Menu::default();
        let rows = (0..worlds)
            .map(|index| Row {
                title: format!("World {index}"),
                details: played(0),
                enabled: true,
            })
            .collect();
        menu.show_list(Page::Worlds, rows, None);
        menu.page = page;
        menu
    }

    fn rows(menu: &Menu) -> Vec<usize> {
        Layout::widgets(menu, DESKTOP)
            .1
            .iter()
            .filter_map(|widget| match widget.action {
                Action::Row(index) => Some(index),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn widgets_and_sliders_follow_scaled_safe_area_layout() {
        let pages = [
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
            Page::Tools,
            Page::Chat,
        ];
        for size in [
            [1000.0, 600.0],
            [320.0, 480.0],
            [700.0, 300.0],
            [640.0, 480.0],
        ] {
            for page in pages {
                for keyboard in [Keyboard::Physical, Keyboard::OnScreen, Keyboard::None] {
                    let platform = Platform {
                        keyboard,
                        ..DESKTOP
                    };
                    let mut menu = menu(page, 8);
                    menu.session = Session::Host { lan: None };
                    let layout = Layout::new(&menu, platform, size, [0.0; 4]);
                    assert!(layout.content[1] + layout.height * layout.scale <= layout.footer_y);
                    for widget in &layout.widgets {
                        let point = [
                            layout.content[0] + (widget.x + widget.width / 2.0) * layout.scale,
                            layout.content[1] + (widget.y + widget.height / 2.0) * layout.scale,
                        ];
                        assert_eq!(
                            layout.action_at(point),
                            widget.enabled.then_some(widget.action)
                        );
                    }
                    assert_eq!(
                        layout.slider_fraction([layout.content[0] - 100.0, 0.0]),
                        0.0
                    );
                    assert_eq!(layout.slider_fraction([size[0] + 100.0, 0.0]), 1.0);
                    let x = layout.content[0] + (WIDE_X + WIDE_WIDTH * 0.75) * layout.scale;
                    assert!((layout.slider_fraction([x, 0.0]) - 0.75).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn world_buttons_need_a_selected_world_and_rows_scroll() {
        let empty = menu(Page::Worlds, 0);
        assert_eq!(empty.selected, None);
        let layout = Layout::new(&empty, DESKTOP, [1000.0, 800.0], [0.0; 4]);
        let enabled: Vec<_> = layout.actions().collect();
        assert_eq!(enabled, [Action::CreateWorld, Action::Back]);

        let mut menu = menu(Page::Worlds, 8);
        assert_eq!(menu.selected_row().unwrap().title, "World 0");
        assert_eq!(rows(&menu), [0, 1, 2, 3, 4]);
        menu.scroll_steps(-10);
        assert_eq!(rows(&menu), [3, 4, 5, 6, 7]);
        menu.scroll_steps(1);
        assert_eq!(rows(&menu), [2, 3, 4, 5, 6]);
        menu.select_step(-1);
        assert_eq!(menu.selected, Some(0));
        assert_eq!(rows(&menu), [0, 1, 2, 3, 4]);
        menu.select_step(10);
        assert_eq!(menu.selected_row().unwrap().title, "World 7");
        assert_eq!(rows(&menu), [3, 4, 5, 6, 7]);
        let rows_again = core::mem::take(&mut menu.rows);
        menu.show_list(Page::Worlds, rows_again, Some(6));
        assert_eq!(menu.selected, Some(6));
        assert_eq!(rows(&menu), [2, 3, 4, 5, 6]);
    }

    #[test]
    fn pause_buttons_follow_the_session() {
        let mut menu = menu(Page::Pause, 0);
        let settings = Settings {
            distance: 8,
            show_stats: false,
            volume: MAX_VOLUME,
            auto_jump: true,
        };
        let actions = |menu: &Menu| {
            Layout::widgets(menu, DESKTOP)
                .1
                .iter()
                .map(|widget| (widget.action, widget.enabled))
                .collect::<Vec<_>>()
        };
        menu.session = Session::Guest;
        assert_eq!(menu.label(Action::QuitToTitle, settings), "Disconnect");
        assert!(!actions(&menu).contains(&(Action::OpenToLan, true)));
        menu.session = Session::Host { lan: None };
        assert!(actions(&menu).contains(&(Action::OpenToLan, true)));
        menu.session = Session::Host { lan: Some(47820) };
        assert!(actions(&menu).contains(&(Action::OpenToLan, false)));
        assert_eq!(
            menu.label(Action::OpenToLan, settings),
            "LAN Open on Port 47820"
        );
    }

    #[test]
    fn the_focus_moves_between_widgets_and_through_the_list() {
        let mut menu = menu(Page::Worlds, 8);
        let platform = Platform {
            keyboard: Keyboard::OnScreen,
            ..DESKTOP
        };
        let layout = |menu: &Menu| Layout::new(menu, platform, [640.0, 480.0], [0.0; 4]);
        assert_eq!(menu.focused(&layout(&menu)), Some(Action::Row(0)));
        for _ in 0..6 {
            menu.step(&layout(&menu), [0.0, 1.0]);
        }
        assert_eq!(menu.selected, Some(6));
        assert_eq!(rows(&menu), [2, 3, 4, 5, 6]);
        menu.focus = Some(Action::PlayWorld);
        menu.step(&layout(&menu), [1.0, 0.0]);
        assert_eq!(menu.focused(&layout(&menu)), Some(Action::CreateWorld));
        menu.show(Page::Create, String::new());
        menu.type_key(Action::Key(13), 2);
        menu.type_key(Action::Space, 2);
        menu.type_key(Action::Key(14), 2);
        assert_eq!(menu.text, "q ");
    }

    #[test]
    fn the_on_screen_keyboard_shifts_once_and_caps_lock_shifts_letters() {
        let mut menu = menu(Page::Chat, 0);
        let typed = |menu: &mut Menu, keys: &[Action]| {
            for &key in keys {
                menu.type_key(key, 100);
            }
        };
        // q is the first key of the second row, 1 the second of the first
        let (q, one) = (Action::Key(13), Action::Key(1));
        typed(
            &mut menu,
            &[Action::Shift(false), q, q, Action::Shift(true), one, one],
        );
        assert_eq!(menu.text, "Qq!1");
        menu.text.clear();
        typed(
            &mut menu,
            &[
                Action::CapsLock,
                q,
                one,
                Action::Shift(false),
                q,
                Action::CapsLock,
                q,
            ],
        );
        assert_eq!(menu.text, "Q1qq");
        let layout = Layout::new(
            &menu,
            Platform {
                keyboard: Keyboard::OnScreen,
                ..DESKTOP
            },
            [640.0, 480.0],
            [0.0; 4],
        );
        let keys = layout
            .actions()
            .filter(|action| matches!(action, Action::Key(_)))
            .count();
        assert_eq!(keys, KEYS.iter().map(|keys| keys.len()).sum());
        assert!(layout.actions().any(|action| action == Action::Enter));
        typed(&mut menu, &[Action::Tab]);
        assert!(menu.text.ends_with('\t'));
    }

    #[test]
    fn distances_and_last_played_times_read_naturally() {
        // Short distances get more of the slider.
        for (fraction, distance) in [(0.0, 4), (0.25, 8), (0.5, 26), (0.75, 64), (1.0, 128)] {
            assert_eq!(distance_at(fraction), distance);
        }
        assert!((distance_fraction(64) - 0.75).abs() < 0.0001);
        let values: Vec<_> = (0..=1000)
            .map(|step| distance_at(step as f32 / 1000.0))
            .collect();
        assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(distance_at(-1.0), MIN_DISTANCE);
        assert_eq!(distance_at(2.0), MAX_DISTANCE);
        for distance in MIN_DISTANCE..=MAX_DISTANCE {
            assert_eq!(distance_at(distance_fraction(distance)), distance);
        }
        assert_eq!(played(5), "Played just now");
        assert_eq!(played(60), "Played 1 minute ago");
        assert_eq!(played(7200), "Played 2 hours ago");
        assert_eq!(played(86_400 * 3), "Played 3 days ago");
    }
}
