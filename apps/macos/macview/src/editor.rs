/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The editing actions of a window, sent by its menus, toolbar, and keys.

use objc2::runtime::{AnyObject as Object, Bool, Sel};
use objc2::{msg_send, sel};

use crate::dialogs::{ask_image_size, ask_rotation};
use crate::edit::{self, Adjustments, Preset, Selection, SelectionShape};
use crate::edit_panel::{PanelKind, show_edit_panel};
use crate::editing::can_paste_image;
use crate::headers::*;
use crate::selection_view::Tool;
use crate::toolbar::{COMPARE_ON_SYMBOL, COMPARE_SYMBOL, TOOLS_ITEM, symbol};
use crate::window_controller::WindowController;

/// The tools in the order of the tools toolbar group.
const TOOLS: [Tool; 3] = [Tool::Pan, Tool::Rectangle, Tool::Ellipse];

/// The aspect ratios of the select aspect ratio menu, by tag: the long side over the short one.
pub(crate) const ASPECT_RATIOS: [(&str, f64); 5] = [
    ("Square", 1.0),
    ("4:3", 4.0 / 3.0),
    ("3:2", 1.5),
    ("16:9", 16.0 / 9.0),
    ("5:4", 1.25),
];

impl WindowController {
    /// Returns whether an item can be used now, and shows its state.
    pub(crate) fn validate_item(&self, item: *mut Object, is_menu_item: bool) -> bool {
        let Some(document) = self.document() else {
            return false;
        };
        // SAFETY: AppKit passes a live menu or toolbar item, both of which have an action.
        let action: Sel = unsafe { msg_send![item, action] };
        let overlay = self.overlay();
        let editable = document.is_editable();
        let can_edit = document.can_edit();
        let has_selection = overlay.selection().is_some();

        let tool_actions = [
            sel!(usePanTool:),
            sel!(useRectangleSelection:),
            sel!(useEllipseSelection:),
        ];
        if let Some(index) = tool_actions.iter().position(|tool| *tool == action) {
            set_state(item, overlay.tool() == TOOLS[index]);
            return editable;
        }
        if action == sel!(selectTool:) {
            if !is_menu_item {
                let index = TOOLS.iter().position(|tool| *tool == overlay.tool());
                // SAFETY: The tools item is a toolbar item group.
                unsafe {
                    let _: () = msg_send![item, setSelectedIndex: index.unwrap_or(0) as isize];
                }
            }
            return editable;
        }
        if action == sel!(toggleCompare:) {
            let comparing = document.is_comparing();
            if is_menu_item {
                set_state(item, comparing);
            } else {
                let name = if comparing {
                    COMPARE_ON_SYMBOL
                } else {
                    COMPARE_SYMBOL
                };
                // SAFETY: The compare item is a toolbar item, which retains its image.
                unsafe {
                    let _: () = msg_send![item, setImage: symbol(name, "Compare")];
                }
            }
            return editable
                && document.has_edits()
                && !document.is_previewing()
                && !document.is_busy();
        }
        if action == sel!(selectAll:) || action == sel!(selectAspectRatio:) {
            return editable && !document.is_previewing();
        }
        if action == sel!(deselectAll:) {
            return has_selection && !document.is_previewing();
        }
        if action == sel!(cropToSelection:) {
            return can_edit && has_selection;
        }
        if action == sel!(resetToOriginal:) {
            return can_edit && document.has_edits();
        }
        if action == sel!(paste:) {
            return can_paste_image();
        }
        let edits = [
            sel!(rotate:),
            sel!(rotateLeft:),
            sel!(rotateRight:),
            sel!(rotateHalf:),
            sel!(rotateByAngle:),
            sel!(straighten:),
            sel!(flip:),
            sel!(flipHorizontal:),
            sel!(flipVertical:),
            sel!(resizeImage:),
            sel!(resizeCanvas:),
            sel!(showAdjustments:),
            sel!(showEffects:),
            sel!(applyPreset:),
            sel!(applyGrayscale:),
            sel!(applyInvert:),
        ];
        if edits.contains(&action) {
            return can_edit;
        }
        true
    }

    /// Switches to the tool at `index` of the tools group.
    pub(crate) fn select_tool_at(&self, index: isize) {
        let Some(&tool) = usize::try_from(index)
            .ok()
            .and_then(|index| TOOLS.get(index))
        else {
            return;
        };
        if !self
            .document()
            .is_some_and(|document| document.is_editable())
        {
            return;
        }
        self.overlay().set_tool(tool);
        // Keep the toolbar in step with tools chosen by key or menu.
        // SAFETY: The window and its toolbar are live while the controller is.
        unsafe {
            let toolbar: *mut Object = msg_send![self.window(), toolbar];
            let item = crate::toolbar::visible_item(toolbar, TOOLS_ITEM);
            if !item.is_null() {
                let _: () = msg_send![item, setSelectedIndex: index];
            }
        }
    }

    // Returns the size of the bitmap in pixels.
    fn canvas_size(&self) -> Option<(f64, f64)> {
        let size = self.document()?.canvas_size()?;
        Some((size.width, size.height))
    }

    // Selects the whole bitmap with the active selection shape.
    pub(crate) fn select_all(&self) {
        let Some((width, height)) = self.canvas_size() else {
            return;
        };
        self.select(0.0, 0.0, width, height);
    }

    // Selects a rectangle with the shape of the active tool, switching to the rectangle tool from
    // the pan tool so the selection can be adjusted right away.
    fn select(&self, x: f64, y: f64, width: f64, height: f64) {
        let overlay = self.overlay();
        if overlay.tool() == Tool::Pan {
            self.select_tool_at(1);
        }
        let shape = match overlay.tool() {
            Tool::Ellipse => SelectionShape::Ellipse,
            _ => SelectionShape::Rectangle,
        };
        overlay.set_selection(Some(Selection {
            shape,
            x,
            y,
            width,
            height,
            feather: 0.0,
        }));
    }

    // Selects the largest centered area with the aspect ratio of the menu item, oriented like the
    // bitmap.
    pub(crate) fn select_aspect_ratio(&self, item: *mut Object) {
        let Some((width, height)) = self.canvas_size() else {
            return;
        };
        // SAFETY: The sender is an item of the aspect ratio menu.
        let tag: isize = unsafe { msg_send![item, tag] };
        let Some(&(_, ratio)) = usize::try_from(tag)
            .ok()
            .and_then(|tag| ASPECT_RATIOS.get(tag))
        else {
            return;
        };
        let ratio = if width >= height { ratio } else { 1.0 / ratio };
        let (selected_width, selected_height) = if width / height > ratio {
            (height * ratio, height)
        } else {
            (width, width / ratio)
        };
        self.select(
            ((width - selected_width) / 2.0).round(),
            ((height - selected_height) / 2.0).round(),
            selected_width.round(),
            selected_height.round(),
        );
    }

    pub(crate) fn crop_to_selection(&self) {
        let (Some(document), Some(selection)) = (self.document(), self.overlay().selection())
        else {
            return;
        };
        document.edit("Crop", move |canvas| edit::crop(canvas, &selection));
    }

    pub(crate) fn rotate_quarters(&self, quarter_turns: u8) {
        if let Some(document) = self.document() {
            let action = match quarter_turns {
                1 => "Rotate Right",
                2 => "Rotate 180\u{b0}",
                _ => "Rotate Left",
            };
            document.edit(action, move |canvas| {
                edit::rotate_quarter(canvas, quarter_turns)
            });
        }
    }

    pub(crate) fn flip(&self, horizontal: bool) {
        if let Some(document) = self.document() {
            if horizontal {
                document.edit("Flip Horizontal", edit::flip_horizontal);
            } else {
                document.edit("Flip Vertical", edit::flip_vertical);
            }
        }
    }

    pub(crate) fn rotate_by_angle(&self) {
        let Some(document) = self.document() else {
            return;
        };
        if !document.can_edit() {
            return;
        }
        let document = document.retained();
        // SAFETY: The window is live, and the document stays retained by the sheet callback.
        unsafe {
            ask_rotation(self.window(), move |degrees| {
                let document = &*document.as_ptr_on_main().cast::<crate::Document>();
                document.edit("Rotate", move |canvas| edit::rotate(canvas, degrees, false));
            });
        }
    }

    // Asks for a new size and resizes the image, or only its canvas.
    pub(crate) fn resize_image(&self, canvas_only: bool) {
        let (Some(document), Some((width, height))) = (self.document(), self.canvas_size()) else {
            return;
        };
        if !document.can_edit() {
            return;
        }
        let (title, information) = if canvas_only {
            (
                "Resize Canvas",
                "The image stays centered. New space is transparent, and parts outside the canvas are cut off.",
            )
        } else {
            ("Resize Image", "The image is scaled to the new size.")
        };
        let document = document.retained();
        // SAFETY: The window is live, and the document stays retained by the sheet callback.
        unsafe {
            ask_image_size(
                self.window(),
                title,
                information,
                (width as u32, height as u32),
                !canvas_only,
                move |width, height| {
                    let document = &*document.as_ptr_on_main().cast::<crate::Document>();
                    if canvas_only {
                        document.edit("Resize Canvas", move |canvas| {
                            edit::resize_canvas(canvas, width, height)
                        });
                    } else {
                        document.edit("Resize Image", move |canvas| {
                            edit::resize(canvas, width, height)
                        });
                    }
                },
            );
        }
    }

    // Shows a popover that previews an edit, limited to the selection.
    pub(crate) fn show_panel(&self, kind: PanelKind) {
        let Some(document) = self.document() else {
            return;
        };
        let selection = match kind {
            PanelKind::Straighten => None,
            _ => self.overlay().selection(),
        };
        // SAFETY: The document and window are live while the controller is.
        let panel = unsafe {
            show_edit_panel(
                kind,
                document as *const crate::Document as *mut Object,
                self.window(),
                selection,
            )
        };
        if panel.is_some() {
            self.set_panel(panel);
        }
    }

    // Applies a preset filter of the filters menu right away.
    pub(crate) fn apply_preset(&self, item: *mut Object) {
        // SAFETY: The sender is an item of the filters menu.
        let tag: isize = unsafe { msg_send![item, tag] };
        let Some(&preset) = usize::try_from(tag)
            .ok()
            .and_then(|tag| Preset::ALL.get(tag))
        else {
            return;
        };
        self.apply_adjustments(preset.name(), preset.adjustments());
    }

    // Turns the image gray or inverts its colors right away.
    pub(crate) fn apply_color(&self, grayscale: bool) {
        let adjustments = Adjustments {
            grayscale,
            invert: !grayscale,
            ..Adjustments::default()
        };
        let action = if grayscale {
            "Grayscale"
        } else {
            "Invert Colors"
        };
        self.apply_adjustments(action, adjustments);
    }

    fn apply_adjustments(&self, action: &'static str, adjustments: Adjustments) {
        let Some(document) = self.document() else {
            return;
        };
        let selection = self.overlay().selection();
        document.edit(action, move |canvas| {
            edit::apply(canvas, &adjustments, selection.as_ref(), 1.0)
        });
    }
}

// Shows a check mark on a menu item.
fn set_state(item: *mut Object, on: bool) {
    // SAFETY: Menu items and toolbar items both respond to state through this check.
    unsafe {
        let has_state: Bool = msg_send![item, respondsToSelector: sel!(setState:)];
        if has_state.as_bool() {
            let state = if on {
                NS_CONTROL_STATE_VALUE_ON
            } else {
                NS_CONTROL_STATE_VALUE_OFF
            };
            let _: () = msg_send![item, setState: state];
        }
    }
}
