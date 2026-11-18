/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Editing in the media view: the edit controls in the header bar, the card with the controls
//! of the open tool, the preview and the crop rectangle or selection over the visible area.

use std::cell::RefCell;

use bcanvas::{CanvasRenderingContext2d, OffscreenCanvas};
use bwindow::WindowId;
use imageview_core::Media;
use imageview_core::edit::{Control, Selection, Shape, Tool};

use super::headers::*;
use super::tool::ToolPanel;
use super::winui::{Bitmap, Mark};
use super::{MediaView, State, send};
use crate::AppEvent;
use crate::viewport::Viewport;

/// The edit state of a view.
#[derive(Default)]
pub(super) struct EditState {
    // The crop rectangle or selection over the visible area.
    overlay: RefCell<Layer>,
    // The controls of the open edit tool.
    tool: RefCell<Option<ToolPanel>>,
}

// A bitmap of the visible area drawn with a canvas, both kept to draw it again.
#[derive(Default)]
struct Layer {
    canvas: Option<OffscreenCanvas>,
    bitmap: Option<Bitmap>,
}

impl Layer {
    // Draws an area of logical pixels at a display scale, returns the bitmap with its size in
    // logical pixels.
    fn draw(
        &mut self,
        (width, height): (f64, f64),
        scale: f64,
        paint: impl FnOnce(&mut CanvasRenderingContext2d<'_>),
    ) -> Option<(&Bitmap, (f64, f64))> {
        let logical = (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32);
        let scale = scale as f32;
        let canvas = match self.canvas.take() {
            Some(canvas)
                if canvas.logical_width() == logical.0 as f32
                    && canvas.logical_height() == logical.1 as f32
                    && canvas.scale_factor() == scale =>
            {
                canvas
            }
            _ => OffscreenCanvas::with_scale_factor(logical.0, logical.1, scale),
        };
        let canvas = self.canvas.insert(canvas);
        // The canvas rounds its pixels up, which the bitmap shows one to one: its logical size
        // would shrink it by a fraction of a pixel, which XAML smooths.
        let size = (canvas.width(), canvas.height());
        let shown = (size.0 as f32 / scale, size.1 as f32 / scale);
        canvas.draw(|context| {
            // A reused canvas still has what it drew before.
            context.clear_rect(0.0, 0.0, shown.0, shown.1);
            paint(context);
        });
        if self
            .bitmap
            .as_ref()
            .is_none_or(|bitmap| bitmap.size() != size)
        {
            self.bitmap = Bitmap::new(size.0, size.1);
        }
        let bitmap = self.bitmap.as_ref()?;
        bitmap.write_rgba(canvas.pixels());
        Some((bitmap, (f64::from(shown.0), f64::from(shown.1))))
    }

    // Frees the canvas and bitmap, returns whether there was a bitmap to hide.
    fn clear(&mut self) -> bool {
        self.canvas = None;
        self.bitmap.take().is_some()
    }
}

impl MediaView {
    /// Shows the edit controls in the header bar, or the view controls again.
    pub(crate) fn set_editing(&mut self, editing: bool) {
        self.close_tool();
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_editing(editing);
        }
        // Only the shown page is edited, so the other pages and the information are hidden
        // meanwhile, the media fits the room that is left.
        let shown = self.state.panels.width();
        self.state.panels.set_hidden(editing);
        self.state.show_panels(self.state.panels.width() != shown);
    }

    /// Shows the controls of a tool in a card over the image, and the crop rectangle for a crop.
    /// Changes of the controls go to `window`. Reset goes back to the defaults.
    pub(crate) fn show_tool(
        &mut self,
        window: WindowId,
        tool: Tool,
        controls: &[Control],
        values: &[f32],
        defaults: &[f32],
        _action: &str,
    ) {
        self.close_tool();
        // Without a card there is no tool that closing would remove the crop rectangle of.
        let Ok(panel) = ToolPanel::new(window, tool, controls, values, defaults) else {
            return;
        };
        if tool == Tool::Crop {
            self.state
                .change_marks(|viewport| viewport.set_cropping(true));
        }
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_tool(Some(panel.content()));
        }
        *self.state.edit.tool.borrow_mut() = Some(panel);
    }

    /// Shows changed values of the open tool.
    pub(crate) fn set_tool_values(&self, values: &[f32]) {
        if let Some(panel) = self.state.edit.tool.borrow().as_ref() {
            panel.set_values(values);
        }
    }

    /// Closes the controls and crop rectangle of the open tool.
    pub(crate) fn close_tool(&mut self) {
        if self.state.edit.tool.borrow_mut().take().is_none() {
            return;
        }
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_tool(None);
            island.focus();
        }
        self.state
            .change_marks(|viewport| viewport.set_cropping(false));
    }

    /// Makes drags draw selections of a shape, or selects the whole image again, shown as on in
    /// the header bar.
    pub(crate) fn set_selecting(&self, shape: Option<Shape>) {
        self.state
            .change_marks(|viewport| viewport.set_selecting(shape));
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_toggled(match shape {
                None => "select.whole",
                Some(Shape::Rectangle) => "select.rectangle",
                Some(Shape::Ellipse) => "select.ellipse",
            });
        }
    }

    /// The selected part of the media.
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.state.viewport.borrow().selection()
    }

    /// Selects everything outside the selection instead.
    pub(crate) fn invert_selection(&self) {
        self.state.change_marks(Viewport::invert_selection);
    }

    /// Puts a PNG image on the clipboard, in the format of that name.
    pub(crate) fn copy_png(&self, png: &[u8]) {
        let format = w!("PNG");
        // SAFETY: The clipboard takes over the global memory once it has it, which is freed
        // here otherwise.
        unsafe {
            if OpenClipboard(self.state.window.get()) == 0 {
                return;
            }
            EmptyClipboard();
            let memory = GlobalAlloc(GMEM_MOVEABLE, png.len());
            let target = GlobalLock(memory);
            if !target.is_null() {
                std::ptr::copy_nonoverlapping(png.as_ptr(), target.cast(), png.len());
                GlobalUnlock(memory);
                if SetClipboardData(RegisterClipboardFormatW(format.as_ptr()), memory).is_null() {
                    GlobalFree(memory);
                }
            } else if !memory.is_null() {
                GlobalFree(memory);
            }
            CloseClipboard();
        }
    }

    /// Shows a preview of any size in the place of the image, or the image again.
    pub(crate) fn set_preview(&self, preview: Option<Media>) {
        self.state.viewport.borrow_mut().set_preview(preview);
        self.state.stale.set(true);
        self.state.render(self.state.zoom_factor());
    }

    /// Places the crop rectangle at a position with a size in pixels.
    pub(crate) fn set_crop_rect(&self, rect: (u32, u32, u32, u32)) {
        self.state.change_marks(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_rect(rect);
            }
        });
    }

    /// Makes the crop rectangle keep a width divided by height, or any shape.
    pub(crate) fn set_crop_aspect(&self, aspect: Option<f64>) {
        self.state.change_marks(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_aspect(aspect);
            }
        });
    }

    /// The crop rectangle in pixels, or all of the media without one.
    pub(crate) fn crop_rect(&self) -> (u32, u32, u32, u32) {
        let mut viewport = self.state.viewport.borrow_mut();
        let (width, height) = viewport.media().size();
        viewport
            .crop()
            .map_or((0, 0, width as u32, height as u32), |crop| crop.rect())
    }

    /// The longest side of the view in device pixels, which previews do not need to exceed.
    pub(crate) fn longest_pixels(&self) -> u32 {
        let (width, height) = self.state.visible();
        (width.max(height) * self.state.scale()).ceil().max(1.0) as u32
    }

    /// Shows a progress bar while an edit is applied.
    pub(crate) fn set_busy(&self, busy: bool) {
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_spinner(busy || !self.state.loaded.get());
        }
    }
}

impl State {
    // Takes a drag that crops or selects, returns whether it does.
    pub(super) fn mark(&self, mark: Mark) -> bool {
        self.follow();
        let marked = {
            let mut viewport = self.viewport.borrow_mut();
            match mark {
                Mark::Begin(pointer) => viewport.mark_begin(pointer),
                Mark::Drag(pointer, square) => viewport.mark_drag(pointer, square),
                Mark::End => {
                    let marked = viewport.mark_end();
                    // A click without a drag goes back to editing the whole image, the menu
                    // follows a drawn selection.
                    if marked && viewport.selecting().is_some() && viewport.selection().is_none() {
                        send(AppEvent::Action("select.whole"));
                    } else if marked {
                        send(AppEvent::Action("select.changed"));
                    }
                    marked
                }
            }
        };
        if marked {
            self.draw_marks();
        }
        marked
    }

    // Changes the crop rectangle or selection and shows it.
    fn change_marks(&self, change: impl FnOnce(&mut Viewport)) {
        change(&mut self.viewport.borrow_mut());
        self.draw_marks();
    }

    // Draws the crop rectangle or selection over the visible area, or hides the overlay.
    pub(super) fn draw_marks(&self) {
        self.follow();
        let island = self.island.borrow();
        let Some(island) = island.as_ref() else {
            return;
        };
        let viewport = self.viewport.borrow();
        let mut overlay = self.edit.overlay.borrow_mut();
        if viewport.has_marks() {
            island.set_overlay(overlay.draw(self.visible(), self.scale(), |context| {
                viewport.paint_marks(context);
            }));
        } else if overlay.clear() {
            island.set_overlay(None);
        }
    }
}
