/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The native scroll bars of a window: its own horizontal and vertical scroll bars in the
//! non-client area, so the client area with the content and its layers narrows instead of
//! being covered. They scroll with their arrows, tracks and thumbs and with the wheel and
//! precision touchpads, with the look of every Windows version and dark in the dark theme.

use std::cell::Cell;
use std::ptr::null;

use super::headers::{BOOL, GetClientRect, HWND, RECT};
use crate::{LogicalPoint, LogicalSize, Theme};

pub(super) const WM_HSCROLL: u32 = 0x0114;
pub(super) const WM_VSCROLL: u32 = 0x0115;
pub(super) const WM_MOUSEWHEEL: u32 = 0x020a;
pub(super) const WM_MOUSEHWHEEL: u32 = 0x020e;
const SB_HORZ: i32 = 0;
const SB_VERT: i32 = 1;
const SB_BOTH: i32 = 3;
const SB_LINEUP: u16 = 0;
const SB_LINEDOWN: u16 = 1;
const SB_PAGEUP: u16 = 2;
const SB_PAGEDOWN: u16 = 3;
const SB_THUMBTRACK: u16 = 5;
const SB_TOP: u16 = 6;
const SB_BOTTOM: u16 = 7;
const SIF_RANGE: u32 = 0x1;
const SIF_PAGE: u32 = 0x2;
const SIF_POS: u32 = 0x4;
const SIF_TRACKPOS: u32 = 0x10;
const WHEEL_DELTA: f32 = 120.0;
// The distance of a scroll bar arrow or a wheel notch in logical pixels.
const LINE: f32 = 40.0;

#[repr(C)]
#[derive(Default)]
struct ScrollInfo {
    cb_size: u32,
    f_mask: u32,
    n_min: i32,
    n_max: i32,
    n_page: u32,
    n_pos: i32,
    n_track_pos: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetScrollInfo(hwnd: HWND, bar: i32, info: *const ScrollInfo, redraw: BOOL) -> i32;
    fn GetScrollInfo(hwnd: HWND, bar: i32, info: *mut ScrollInfo) -> BOOL;
    fn ShowScrollBar(hwnd: HWND, bar: i32, show: BOOL) -> BOOL;
}

#[link(name = "uxtheme")]
unsafe extern "system" {
    fn SetWindowTheme(hwnd: HWND, app_name: *const u16, id_list: *const u16) -> i32;
}

#[derive(Default)]
pub(super) struct Scroll {
    size: Cell<Option<LogicalSize>>,
    offset: Cell<(f32, f32)>,
    // The theme of the scroll bars, set again only when it changes, because theming the
    // window sends WM_THEMECHANGED.
    theme: Cell<Option<Theme>>,
}

impl Scroll {
    pub(super) const fn offset(&self) -> LogicalPoint {
        let (x, y) = self.offset.get();
        LogicalPoint::new(x, y)
    }

    /// Show scroll bars for content of a size, or hide them with none.
    pub(super) fn set_size(&self, hwnd: HWND, dpi: u32, theme: Theme, size: Option<LogicalSize>) {
        let previous = self.size.replace(size);
        if previous == size {
            return;
        }
        if size.is_none() {
            self.offset.set((0.0, 0.0));
            unsafe { ShowScrollBar(hwnd, SB_BOTH, 0) };
            return;
        }
        self.set_theme(hwnd, theme);
        self.update(hwnd, dpi);
    }

    /// Fit the scroll bars to the client area, which a scroll bar showing or hiding changes.
    /// Returns the offset when it changed.
    pub(super) fn update(&self, hwnd: HWND, dpi: u32) -> Option<LogicalPoint> {
        let size = self.size.get()?;
        let scale = dpi as f32 / 96.0;
        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) };
        let page = |pixels: i32| pixels.max(0) as f32 / scale;
        let (width, height) = (page(client.right), page(client.bottom));
        let (x, y) = self.offset.get();
        let offset = (
            x.clamp(0.0, (size.width - width).max(0.0)),
            y.clamp(0.0, (size.height - height).max(0.0)),
        );
        for (bar, content, visible, position) in [
            (SB_HORZ, size.width, client.right, offset.0),
            (SB_VERT, size.height, client.bottom, offset.1),
        ] {
            let info = ScrollInfo {
                cb_size: size_of::<ScrollInfo>() as u32,
                f_mask: SIF_RANGE | SIF_PAGE | SIF_POS,
                n_min: 0,
                n_max: (content * scale).round() as i32 - 1,
                n_page: visible.max(0) as u32,
                n_pos: (position * scale).round() as i32,
                n_track_pos: 0,
            };
            // A bar whose page covers its range hides, which resizes the client area again.
            unsafe { SetScrollInfo(hwnd, bar, &info, 1) };
        }
        (self.offset.replace(offset) != offset).then(|| self.offset())
    }

    /// Scroll to an offset within the content, returns it when it changed.
    pub(super) fn scroll_to(
        &self,
        hwnd: HWND,
        dpi: u32,
        offset: LogicalPoint,
    ) -> Option<LogicalPoint> {
        self.size.get()?;
        let previous = self.offset.replace((offset.x, offset.y));
        let clamped = self.update(hwnd, dpi);
        let current = self.offset.get();
        (clamped.is_some() || current != previous).then(|| self.offset())
    }

    /// Give the scroll bars the dark look in the dark theme from Windows 10, the light one
    /// otherwise, when it changed.
    pub(super) fn set_theme(&self, hwnd: HWND, theme: Theme) {
        if self.size.get().is_none() || self.theme.replace(Some(theme)) == Some(theme) {
            return;
        }
        let name = if theme == Theme::Dark {
            "DarkMode_Explorer"
        } else {
            "Explorer"
        };
        let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
        unsafe { SetWindowTheme(hwnd, name.as_ptr(), null()) };
    }

    /// Handle the scroll bar and wheel messages while scrolling, with the new offset when it
    /// changed.
    pub(super) fn handle(
        &self,
        hwnd: HWND,
        dpi: u32,
        message: u32,
        wparam: usize,
    ) -> Option<Option<LogicalPoint>> {
        self.size.get()?;
        let scale = dpi as f32 / 96.0;
        let (x, y) = self.offset.get();
        let wheel = (wparam >> 16) as u16 as i16 as f32 / WHEEL_DELTA * LINE;
        let offset = match message {
            WM_MOUSEWHEEL => LogicalPoint::new(x, y - wheel),
            WM_MOUSEHWHEEL => LogicalPoint::new(x + wheel, y),
            WM_HSCROLL => LogicalPoint::new(self.bar_position(hwnd, SB_HORZ, x, scale, wparam), y),
            WM_VSCROLL => LogicalPoint::new(x, self.bar_position(hwnd, SB_VERT, y, scale, wparam)),
            _ => return None,
        };
        Some(self.scroll_to(hwnd, dpi, offset))
    }

    // The position a scroll bar request scrolls to.
    fn bar_position(&self, hwnd: HWND, bar: i32, position: f32, scale: f32, wparam: usize) -> f32 {
        let mut info = ScrollInfo {
            cb_size: size_of::<ScrollInfo>() as u32,
            f_mask: SIF_RANGE | SIF_PAGE | SIF_TRACKPOS,
            ..Default::default()
        };
        unsafe { GetScrollInfo(hwnd, bar, &mut info) };
        let page = info.n_page as f32 / scale;
        match wparam as u16 {
            SB_LINEUP => position - LINE,
            SB_LINEDOWN => position + LINE,
            SB_PAGEUP => position - page,
            SB_PAGEDOWN => position + page,
            SB_THUMBTRACK => info.n_track_pos as f32 / scale,
            SB_TOP => 0.0,
            SB_BOTTOM => f32::MAX,
            _ => position,
        }
    }
}
