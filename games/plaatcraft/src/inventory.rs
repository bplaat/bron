/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A Minecraft style hotbar with nine quick select slots and a scrollable creative
//! inventory of every block. bcanvas draws the frames and text, the block icons are
//! drawn on top by the block preview pipeline, see [`Inventory::icons`].

use std::time::{Duration, Instant};

use bcanvas::{CanvasRenderingContext2d, Color, TextAlign, TextBaseline};
use bwindow::{LogicalInsets, LogicalPoint, LogicalSize};

use crate::hud::{Cached, Hud, MARGIN, Painter};
use crate::world;

pub(crate) const SLOTS: usize = 9;
const DEFAULT_SLOTS: [u8; SLOTS] = [1, 2, 3, 28, 27, 62, 6, 9, 4];
const COLUMNS: usize = 9;
const NAME_DURATION: Duration = Duration::from_secs(2);
// Room around the slots for the selection frame.
const FRAME: f32 = 3.0;
const TAP_DISTANCE: f32 = 8.0;
const NAME_WIDTH: f32 = 240.0;
const NAME_HEIGHT: f32 = 22.0;

pub(crate) fn name(material: u8) -> &'static str {
    world::MATERIALS
        .iter()
        .find(|&&(id, _)| id == material)
        .map_or("Unknown", |&(_, name)| name)
}

// MARK: Hotbar
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hotbar {
    pub(crate) slots: [u8; SLOTS],
    pub(crate) selected: usize,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self {
            slots: DEFAULT_SLOTS,
            selected: 0,
        }
    }
}

impl Hotbar {
    pub(crate) const fn material(&self) -> u8 {
        self.slots[self.selected]
    }

    // Scrolling down selects the next slot, like the mouse wheel in Minecraft.
    pub(crate) const fn cycle(&mut self, steps: i64) {
        self.selected = (self.selected as i64 - steps).rem_euclid(SLOTS as i64) as usize;
    }

    // Selects the slot holding a material, or puts it in the selected slot.
    pub(crate) fn pick(&mut self, material: u8) {
        match self.slots.iter().position(|&slot| slot == material) {
            Some(slot) => self.selected = slot,
            None => self.slots[self.selected] = material,
        }
    }

    // Puts a material in a slot, a slot that already held it gets the replaced material.
    pub(crate) fn assign(&mut self, slot: usize, material: u8) {
        if let Some(other) = self.slots.iter().position(|&held| held == material) {
            self.slots[other] = self.slots[slot];
        }
        self.slots[slot] = material;
    }
}

// MARK: Layout
struct Panel {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    grid_x: f32,
    grid_y: f32,
    rows: usize,
}

// The positions of the hotbar and the open inventory in logical pixels.
pub(crate) struct Layout {
    slot: f32,
    hotbar_x: f32,
    hotbar_y: f32,
    // Touch screens get an extra slot after the hotbar that opens the inventory.
    more: bool,
    panel: Option<Panel>,
}

const fn total_rows() -> usize {
    world::MATERIALS.len().div_ceil(COLUMNS)
}

impl Layout {
    // Touch screens pass the band between their controls, the hotbar shrinks to fit it.
    pub(crate) fn new(
        size: LogicalSize,
        safe_area: LogicalInsets,
        more: bool,
        open: bool,
        band: Option<(f32, f32)>,
    ) -> Self {
        let count = SLOTS as f32 + f32::from(u8::from(more));
        let (left, right) = band.unwrap_or((0.0, size.width));
        let slot = (size.width * 0.05)
            .clamp(28.0, 44.0)
            .min((right - left) / count)
            .max(24.0)
            .floor();
        let hotbar_x = (left + (right - left - count * slot) / 2.0).round();
        let hotbar_y = size.height - safe_area.bottom - MARGIN / 2.0 - slot;
        let panel = open.then(|| {
            let padding = (slot * 0.3).round();
            let title = (slot * 0.6).round();
            let scrollbar = 10.0;
            let top = safe_area.top + MARGIN / 2.0;
            let available = hotbar_y - FRAME - MARGIN / 2.0 - top;
            let rows = (((available - title - padding * 2.0) / slot)
                .floor()
                .max(1.0) as usize)
                .min(total_rows());
            let width = COLUMNS as f32 * slot + padding * 3.0 + scrollbar;
            let height = title + rows as f32 * slot + padding * 2.0;
            let x = ((size.width - width) / 2.0).round();
            let y = (top + (available - height) / 2.0).round();
            Panel {
                x,
                y,
                width,
                height,
                grid_x: x + padding,
                grid_y: y + padding + title,
                rows,
            }
        });
        Self {
            slot,
            hotbar_x,
            hotbar_y,
            more,
            panel,
        }
    }

    fn max_scroll(&self) -> f32 {
        self.panel
            .as_ref()
            .map_or(0.0, |panel| (total_rows() - panel.rows) as f32 * self.slot)
    }

    pub(crate) fn hotbar_slot_at(&self, position: LogicalPoint) -> Option<usize> {
        let column = ((position.x - self.hotbar_x) / self.slot).floor();
        ((self.hotbar_y..self.hotbar_y + self.slot).contains(&position.y)
            && (0.0..SLOTS as f32).contains(&column))
        .then_some(column as usize)
    }

    pub(crate) fn more_at(&self, position: LogicalPoint) -> bool {
        let x = self.hotbar_x + SLOTS as f32 * self.slot;
        self.more
            && (x..x + self.slot).contains(&position.x)
            && (self.hotbar_y..self.hotbar_y + self.slot).contains(&position.y)
    }

    pub(crate) fn in_panel(&self, position: LogicalPoint) -> bool {
        self.panel.as_ref().is_some_and(|panel| {
            (panel.x..panel.x + panel.width).contains(&position.x)
                && (panel.y..panel.y + panel.height).contains(&position.y)
        })
    }

    fn item_at(&self, position: LogicalPoint, scroll: f32) -> Option<u8> {
        let panel = self.panel.as_ref()?;
        let column = ((position.x - panel.grid_x) / self.slot).floor();
        let y = position.y - panel.grid_y;
        if !(0.0..COLUMNS as f32).contains(&column)
            || !(0.0..panel.rows as f32 * self.slot).contains(&y)
        {
            return None;
        }
        let row = ((y + scroll) / self.slot).floor() as usize;
        world::MATERIALS
            .get(row * COLUMNS + column as usize)
            .map(|&(material, _)| material)
    }
}

// MARK: Inventory
// A block icon in logical pixels, clipped to a rectangle.
pub(crate) struct Icon {
    pub(crate) material: u8,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) size: f32,
    pub(crate) clip: (f32, f32, f32, f32),
}

pub(crate) enum Action {
    Select(usize),
    Assign(usize, u8),
    Toggle,
}

#[derive(Default)]
pub(crate) struct Inventory {
    pub(crate) open: bool,
    scroll: f32,
    hover: Option<u8>,
    // A touch dragging the inventory, with its start and last position.
    drag: Option<(u64, LogicalPoint, LogicalPoint)>,
    shown: Option<(u8, Instant)>,
    hotbar: Option<Cached>,
    panel: Option<Cached>,
    name: Option<Cached>,
}

impl Inventory {
    pub(crate) const fn toggle(&mut self) {
        self.open = !self.open;
        self.hover = None;
        self.drag = None;
    }

    pub(crate) fn hover(&mut self, layout: &Layout, position: Option<LogicalPoint>) {
        self.hover =
            position.and_then(|position| layout.item_at(position, self.scroll_offset(layout)));
    }

    pub(crate) const fn hovered(&self) -> Option<u8> {
        self.hover
    }

    // Wheel steps scroll one row each, positive steps scroll up.
    pub(crate) fn scroll_steps(&mut self, layout: &Layout, steps: i64) {
        self.scroll_by(layout, -(steps as f32) * layout.slot);
    }

    fn scroll_by(&mut self, layout: &Layout, delta: f32) {
        self.scroll = (self.scroll_offset(layout) + delta).clamp(0.0, layout.max_scroll());
    }

    // Resizing can reveal more rows, reducing the valid scroll range.
    fn scroll_offset(&self, layout: &Layout) -> f32 {
        self.scroll.min(layout.max_scroll())
    }

    // A click or tap: hotbar slots select, inventory blocks go into the selected slot.
    pub(crate) fn click(
        &self,
        layout: &Layout,
        position: LogicalPoint,
        selected: usize,
    ) -> Option<Action> {
        if let Some(slot) = layout.hotbar_slot_at(position) {
            Some(Action::Select(slot))
        } else if layout.more_at(position) {
            Some(Action::Toggle)
        } else if let Some(material) = layout.item_at(position, self.scroll_offset(layout)) {
            Some(Action::Assign(selected, material))
        } else {
            (self.open && !layout.in_panel(position)).then_some(Action::Toggle)
        }
    }

    // Touches drag the open inventory and tap when they barely moved.
    pub(crate) const fn touch_start(&mut self, identifier: u64, position: LogicalPoint) {
        if self.drag.is_none() {
            self.drag = Some((identifier, position, position));
        }
    }

    pub(crate) fn touch_move(&mut self, layout: &Layout, identifier: u64, position: LogicalPoint) {
        if let Some((id, _, last)) = self.drag
            && id == identifier
        {
            self.scroll_by(layout, last.y - position.y);
            self.drag = self.drag.map(|(id, start, _)| (id, start, position));
        }
    }

    pub(crate) fn touch_end(
        &mut self,
        layout: &Layout,
        identifier: u64,
        position: LogicalPoint,
        selected: usize,
    ) -> Option<Action> {
        let (id, start, _) = self.drag?;
        if id != identifier {
            return None;
        }
        self.drag = None;
        ((position.x - start.x).hypot(position.y - start.y) < TAP_DISTANCE)
            .then(|| self.click(layout, position, selected))
            .flatten()
    }

    pub(crate) fn touch_cancel(&mut self, identifier: u64) {
        if self.drag.is_some_and(|(id, ..)| id == identifier) {
            self.drag = None;
        }
    }

    // Shows the name of a newly selected block above the hotbar for a while.
    pub(crate) fn show_name(&mut self, material: u8) {
        self.shown = Some((material, Instant::now()));
    }

    pub(crate) fn icons(&self, layout: &Layout, hotbar: &Hotbar) -> Vec<Icon> {
        let inset = (layout.slot * 0.14).round();
        let size = layout.slot - inset * 2.0;
        let screen = (0.0, 0.0, f32::MAX, f32::MAX);
        let mut icons = hotbar
            .slots
            .iter()
            .enumerate()
            .map(|(index, &material)| Icon {
                material,
                x: layout.hotbar_x + index as f32 * layout.slot + inset,
                y: layout.hotbar_y + inset,
                size,
                clip: screen,
            })
            .collect::<Vec<_>>();
        if let Some(panel) = &layout.panel {
            let scroll = self.scroll_offset(layout);
            let clip = (
                panel.grid_x,
                panel.grid_y,
                COLUMNS as f32 * layout.slot,
                panel.rows as f32 * layout.slot,
            );
            let first = (scroll / layout.slot).floor() as usize;
            for row in first..(first + panel.rows + 1).min(total_rows()) {
                for column in 0..COLUMNS {
                    let Some(&(material, _)) = world::MATERIALS.get(row * COLUMNS + column) else {
                        break;
                    };
                    icons.push(Icon {
                        material,
                        x: panel.grid_x + column as f32 * layout.slot + inset,
                        y: panel.grid_y + row as f32 * layout.slot - scroll + inset,
                        size,
                        clip,
                    });
                }
            }
        }
        icons
    }

    // Draws the sprites again when what they show changed.
    pub(crate) fn prepare(&mut self, painter: &Painter<'_>, layout: &Layout, hotbar: &Hotbar) {
        if self.open {
            self.scroll_by(layout, 0.0);
        }
        let slot = layout.slot;
        let count = SLOTS + usize::from(layout.more);
        let selected = hotbar.selected;
        painter.paint(
            &mut self.hotbar,
            [slot.to_bits(), count as u32, selected as u32, 0, 0],
            (count as f32 * slot + FRAME * 2.0, slot + FRAME * 2.0),
            |ctx| draw_hotbar(ctx, slot, count, selected),
        );

        if let Some(panel) = &layout.panel {
            let (grid, rows) = ((panel.grid_x - panel.x, panel.grid_y - panel.y), panel.rows);
            let (scroll, max_scroll, hover) =
                (self.scroll_offset(layout), layout.max_scroll(), self.hover);
            painter.paint(
                &mut self.panel,
                [
                    slot.to_bits(),
                    rows as u32,
                    scroll.to_bits(),
                    hover.map_or(u32::MAX, u32::from),
                    0,
                ],
                (panel.width, panel.height),
                |ctx| draw_panel(ctx, slot, grid, rows, scroll, max_scroll, hover),
            );
        }

        if let Some((material, _)) = self.shown {
            painter.paint(
                &mut self.name,
                [0, 0, u32::from(material), 0, 0],
                (NAME_WIDTH, NAME_HEIGHT),
                |ctx| {
                    ctx.set_font("sans-serif", 15.0);
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    let (x, y) = (ctx.width() / 2.0, ctx.height() / 2.0);
                    ctx.set_fill_style(Color::rgba(0, 0, 0, 170));
                    ctx.fill_text(name(material), x + 1.0, y + 1.0);
                    ctx.set_fill_style(Color::rgb(255, 255, 255));
                    ctx.fill_text(name(material), x, y);
                },
            );
        }
    }

    // Draws the frames below the icons of the same element.
    pub(crate) fn draw_hotbar(
        &self,
        hud: &Hud,
        pass: &mut bwebgpu::RenderPass<'_>,
        layout: &Layout,
        scale: f32,
        target: (u32, u32),
    ) {
        if let Some(cached) = &self.hotbar {
            let position = (
                (layout.hotbar_x - FRAME) * scale,
                (layout.hotbar_y - FRAME) * scale,
            );
            hud.draw(pass, cached.sprite(), position, target);
        }
        if let (Some((_, shown)), Some(cached)) = (self.shown, &self.name)
            && shown.elapsed() < NAME_DURATION
            && !self.open
        {
            let x = (layout.hotbar_x + (SLOTS as f32 * layout.slot - NAME_WIDTH) / 2.0) * scale;
            let y = (layout.hotbar_y - FRAME - 26.0) * scale;
            hud.draw(pass, cached.sprite(), (x, y), target);
        }
    }

    pub(crate) fn draw_panel(
        &self,
        hud: &Hud,
        pass: &mut bwebgpu::RenderPass<'_>,
        layout: &Layout,
        scale: f32,
        target: (u32, u32),
    ) {
        if let (Some(panel), Some(cached)) = (&layout.panel, &self.panel) {
            hud.draw(
                pass,
                cached.sprite(),
                (panel.x * scale, panel.y * scale),
                target,
            );
        }
    }
}

// MARK: Drawing
// A Minecraft style inset slot: dark top left edges and light bottom right edges.
fn draw_slot(ctx: &mut CanvasRenderingContext2d<'_>, x: f32, y: f32, size: f32, alpha: u8) {
    ctx.set_fill_style(Color::rgba(139, 139, 139, alpha));
    ctx.fill_rect(x, y, size, size);
    ctx.set_fill_style(Color::rgba(55, 55, 55, alpha.saturating_add(40)));
    ctx.fill_rect(x, y, size, 2.0);
    ctx.fill_rect(x, y, 2.0, size);
    ctx.set_fill_style(Color::rgba(255, 255, 255, alpha));
    ctx.fill_rect(x + 2.0, y + size - 2.0, size - 2.0, 2.0);
    ctx.fill_rect(x + size - 2.0, y + 2.0, 2.0, size - 2.0);
}

fn draw_hotbar(ctx: &mut CanvasRenderingContext2d<'_>, slot: f32, count: usize, selected: usize) {
    ctx.set_fill_style(Color::rgba(0, 0, 0, 120));
    ctx.fill_rect(FRAME, FRAME, count as f32 * slot, slot);
    for index in 0..count {
        let x = FRAME + index as f32 * slot;
        draw_slot(ctx, x + 1.0, FRAME + 1.0, slot - 2.0, 150);
        if index < SLOTS {
            ctx.set_font("sans-serif", 10.0);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_fill_style(Color::rgba(255, 255, 255, 150));
            ctx.fill_text(format!("{}", index + 1), x + 4.0, FRAME + 3.0);
        } else {
            // The extra touch slot that opens the inventory.
            ctx.set_font("sans-serif", 18.0);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_style(Color::rgb(255, 255, 255));
            ctx.fill_text("...", x + slot / 2.0, FRAME + slot / 2.0 - 3.0);
        }
    }
    let x = FRAME + selected as f32 * slot;
    ctx.set_line_width(3.0);
    ctx.set_stroke_style(Color::rgb(255, 255, 255));
    ctx.stroke_rect(x - 1.5, FRAME - 1.5, slot + 3.0, slot + 3.0);
    ctx.set_line_width(1.0);
    ctx.set_stroke_style(Color::rgba(0, 0, 0, 160));
    ctx.stroke_rect(x - 3.5, FRAME - 3.5, slot + 7.0, slot + 7.0);
}

fn draw_panel(
    ctx: &mut CanvasRenderingContext2d<'_>,
    slot: f32,
    (grid_x, grid_y): (f32, f32),
    rows: usize,
    scroll: f32,
    max_scroll: f32,
    hover: Option<u8>,
) {
    let (width, height) = (ctx.width(), ctx.height());
    // A raised light gray window with a dark outline, like Minecraft's inventory.
    ctx.set_fill_style(Color::rgb(0, 0, 0));
    ctx.fill_rect(0.0, 0.0, width, height);
    ctx.set_fill_style(Color::rgb(85, 85, 85));
    ctx.fill_rect(2.0, 2.0, width - 4.0, height - 4.0);
    ctx.set_fill_style(Color::rgb(255, 255, 255));
    ctx.fill_rect(2.0, 2.0, width - 6.0, height - 6.0);
    ctx.set_fill_style(Color::rgb(198, 198, 198));
    ctx.fill_rect(4.0, 4.0, width - 8.0, height - 8.0);

    ctx.set_font("sans-serif", 14.0);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_fill_style(Color::rgb(63, 63, 63));
    let title_y = grid_y / 2.0 + 2.0;
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("Blocks", grid_x, title_y);
    if let Some(material) = hover {
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(name(material), grid_x + COLUMNS as f32 * slot, title_y);
    }

    let grid_height = rows as f32 * slot;
    ctx.save();
    ctx.begin_path();
    ctx.rect(grid_x, grid_y, COLUMNS as f32 * slot, grid_height);
    ctx.clip();
    let first = (scroll / slot).floor() as usize;
    for row in first..(first + rows + 1).min(total_rows()) {
        for column in 0..COLUMNS {
            let Some(&(material, _)) = world::MATERIALS.get(row * COLUMNS + column) else {
                break;
            };
            let x = grid_x + column as f32 * slot;
            let y = grid_y + row as f32 * slot - scroll;
            draw_slot(ctx, x, y, slot, 255);
            if hover == Some(material) {
                ctx.set_fill_style(Color::rgba(255, 255, 255, 110));
                ctx.fill_rect(x + 2.0, y + 2.0, slot - 4.0, slot - 4.0);
            }
        }
    }
    ctx.restore();

    // The scrollbar next to the grid.
    let track_x = grid_x * 2.0 + COLUMNS as f32 * slot;
    ctx.set_fill_style(Color::rgb(139, 139, 139));
    ctx.fill_rect(track_x, grid_y, 10.0, grid_height);
    let thumb = (grid_height * grid_height / (grid_height + max_scroll)).max(12.0);
    let thumb_y = grid_y
        + if max_scroll > 0.0 {
            scroll / max_scroll * (grid_height - thumb)
        } else {
            0.0
        };
    ctx.set_fill_style(Color::rgb(255, 255, 255));
    ctx.fill_rect(track_x, thumb_y, 10.0, thumb);
    ctx.set_fill_style(Color::rgb(85, 85, 85));
    ctx.fill_rect(track_x + 2.0, thumb_y + 2.0, 8.0, thumb - 2.0);
    ctx.set_fill_style(Color::rgb(198, 198, 198));
    ctx.fill_rect(track_x + 2.0, thumb_y + 2.0, 6.0, thumb - 4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotbar_cycles_picks_and_assigns_without_duplicates() {
        let mut hotbar = Hotbar::default();
        hotbar.cycle(-1);
        assert_eq!(hotbar.selected, 1);
        hotbar.cycle(2);
        assert_eq!(hotbar.selected, SLOTS - 1);

        hotbar.pick(DEFAULT_SLOTS[2]);
        assert_eq!(hotbar.selected, 2);
        hotbar.pick(world::WATER);
        assert_eq!(hotbar.slots[2], world::WATER);

        // Assigning a material that is already in another slot swaps the two slots.
        hotbar.assign(0, world::WATER);
        assert_eq!(hotbar.slots[0], world::WATER);
        assert_eq!(hotbar.slots[2], DEFAULT_SLOTS[0]);
        assert_eq!(
            hotbar
                .slots
                .iter()
                .filter(|&&slot| slot == world::WATER)
                .count(),
            1
        );
    }

    #[test]
    fn inventory_scrolls_and_clicks_blocks_into_the_selected_slot() {
        let size = LogicalSize::new(800.0, 400.0);
        let layout = Layout::new(size, LogicalInsets::default(), false, true, None);
        let panel = layout.panel.as_ref().unwrap();
        assert!(panel.rows < total_rows());
        assert!(panel.y >= 0.0 && panel.y + panel.height <= layout.hotbar_y);

        let mut inventory = Inventory {
            open: true,
            ..Inventory::default()
        };
        let first = LogicalPoint::new(panel.grid_x + 1.0, panel.grid_y + 1.0);
        assert!(matches!(
            inventory.click(&layout, first, 4),
            Some(Action::Assign(4, 1))
        ));
        inventory.scroll_by(&layout, 1e6);
        assert_eq!(inventory.scroll, layout.max_scroll());
        inventory.hover(&layout, Some(first));
        assert_eq!(
            inventory.hovered(),
            Some(world::MATERIALS[(total_rows() - panel.rows) * COLUMNS].0)
        );

        // The hotbar stays clickable and clicking outside the panel closes it.
        let slot = LogicalPoint::new(layout.hotbar_x + layout.slot * 3.5, layout.hotbar_y + 2.0);
        assert!(matches!(
            inventory.click(&layout, slot, 0),
            Some(Action::Select(3))
        ));
        assert!(matches!(
            inventory.click(&layout, LogicalPoint::new(1.0, 1.0), 0),
            Some(Action::Toggle)
        ));
    }

    #[test]
    fn resizing_keeps_inventory_icons_and_picking_in_the_valid_scroll_range() {
        let layout = |height| {
            Layout::new(
                LogicalSize::new(800.0, height),
                LogicalInsets::default(),
                false,
                true,
                None,
            )
        };
        let short = layout(200.0);
        let tall = layout(400.0);
        assert!(short.max_scroll() > tall.max_scroll());
        let mut inventory = Inventory {
            open: true,
            ..Inventory::default()
        };
        inventory.scroll_by(&short, 1e6);

        let panel = tall.panel.as_ref().unwrap();
        let first = LogicalPoint::new(panel.grid_x + 1.0, panel.grid_y + 1.0);
        let material = world::MATERIALS[(total_rows() - panel.rows) * COLUMNS].0;
        inventory.hover(&tall, Some(first));
        assert_eq!(inventory.hovered(), Some(material));
        assert!(matches!(
            inventory.click(&tall, first, 0),
            Some(Action::Assign(0, selected)) if selected == material
        ));
        let icons = inventory.icons(&tall, &Hotbar::default());
        assert_eq!(icons[SLOTS].material, material);
        assert!(icons[SLOTS].y >= panel.grid_y);

        inventory.scroll_steps(&tall, 1);
        assert_eq!(inventory.scroll, tall.max_scroll() - tall.slot);
    }

    #[test]
    fn touches_tap_the_more_slot_and_drag_the_inventory() {
        let size = LogicalSize::new(800.0, 400.0);
        let closed = Layout::new(size, LogicalInsets::default(), true, false, None);
        let more = LogicalPoint::new(
            closed.hotbar_x + (SLOTS as f32 + 0.5) * closed.slot,
            closed.hotbar_y + 4.0,
        );
        let mut inventory = Inventory::default();
        inventory.touch_start(1, more);
        assert!(matches!(
            inventory.touch_end(&closed, 1, more, 0),
            Some(Action::Toggle)
        ));

        let open = Layout::new(size, LogicalInsets::default(), true, true, None);
        let panel = open.panel.as_ref().unwrap();
        let start = LogicalPoint::new(panel.grid_x + 4.0, panel.grid_y + open.slot);
        let end = LogicalPoint::new(start.x, start.y - 30.0);
        inventory.touch_start(2, start);
        inventory.touch_move(&open, 2, end);
        assert_eq!(inventory.scroll, 30.0);
        assert!(inventory.touch_end(&open, 2, end, 0).is_none());
    }

    #[test]
    fn canceled_inventory_touches_do_not_click_or_stay_held() {
        let mut inventory = Inventory::default();
        let start = LogicalPoint::new(10.0, 20.0);
        inventory.touch_start(1, start);
        inventory.touch_cancel(2);
        assert!(inventory.drag.is_some());
        inventory.touch_cancel(1);
        assert!(inventory.drag.is_none());
        inventory.touch_start(3, start);
        assert_eq!(inventory.drag.map(|(id, ..)| id), Some(3));
    }

    #[test]
    fn touch_hotbar_shrinks_into_the_band_between_the_controls() {
        let size = LogicalSize::new(667.0, 375.0);
        let layout = Layout::new(
            size,
            LogicalInsets::default(),
            true,
            false,
            Some((150.0, 480.0)),
        );
        assert!(layout.hotbar_x >= 150.0);
        assert!(layout.hotbar_x + (SLOTS as f32 + 1.0) * layout.slot <= 480.0);
        assert!(layout.slot < 44.0);
    }
}
