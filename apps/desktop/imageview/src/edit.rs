/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Edit mode of an image window: the editor of its picture, the selection that edits change,
//! the open tool with its preview, and saving. Viewing stays the default, edit actions start
//! editing when they are used.
//!
//! Tools preview on a copy of the picture as large as the window, so sliders follow quickly,
//! and are applied to the full picture in the background. Opening the tool of the last change
//! again sets it up from the picture before it, so it can be changed instead of stacked.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bwindow::WindowId;
#[cfg(not(windows))]
use bwindow::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use imageview_core::edit::{
    DEFAULT_JPEG_QUALITY, DEFAULT_SAVE_EXTENSION, Editor, Filter, Orientation, Picture,
    SAVE_FORMATS, Selection, Shape, Step, Tool, can_save, crop_aspect, is_jpeg,
};
use imageview_core::{FileVersion, Info, Media, decode_pages};

use crate::save::{SaveRequest, ask_save};
use crate::{App, AppEvent, ImageWindow, file_name, title};

// The filters of their actions.
const FILTERS: &[(&str, Filter)] = &[
    ("filter.adjust", Filter::Adjust),
    ("filter.auto_enhance", Filter::AutoEnhance),
    ("filter.grayscale", Filter::Grayscale),
    ("filter.sepia", Filter::Sepia),
    ("filter.invert", Filter::Invert),
    ("filter.threshold", Filter::Threshold),
    ("filter.solarize", Filter::Solarize),
    ("filter.gaussian_blur", Filter::GaussianBlur),
    ("filter.box_blur", Filter::BoxBlur),
    ("filter.motion_blur", Filter::MotionBlur),
    ("filter.radial_blur", Filter::RadialBlur),
    ("filter.sharpen", Filter::Sharpen),
    ("filter.reduce_noise", Filter::ReduceNoise),
    ("filter.pixelate", Filter::Pixelate),
];

// The turns of their actions.
const ORIENTATIONS: &[(&str, Orientation)] = &[
    ("image.rotate_left", Orientation::Rotate270),
    ("image.rotate_right", Orientation::Rotate90),
    ("image.flip_horizontal", Orientation::FlipHorizontal),
    ("image.flip_vertical", Orientation::FlipVertical),
];

// A tool with its popover open.
#[derive(Clone)]
struct OpenTool {
    tool: Tool,
    values: Vec<f32>,
    // The picture the tool changes: the edited one, or the one before the last change when
    // that is set up again.
    base: Arc<Picture>,
    amend: bool,
    selection: Option<Selection>,
}

// A smaller copy of a picture for previews, and the scale it shows the picture at.
struct Proxy {
    base: Arc<Picture>,
    picture: Arc<Picture>,
    scale: f64,
}

/// The editing state of a window.
pub(crate) struct Editing {
    editor: Editor,
    tool: Option<OpenTool>,
    proxy: Option<Proxy>,
    // Increments for every preview, so only the latest one is shown.
    preview: u64,
    // Whether a preview is being made, and whether the values changed since it started.
    previewing: bool,
    stale: bool,
    // The quality JPEG files are saved with.
    jpeg_quality: u8,
}

/// What continues after a save that succeeded.
pub(crate) enum AfterSave {
    Nothing,
    FinishEditing,
    Close,
    Show(PathBuf),
}

/// A save that finished in the background.
pub(crate) struct Saved {
    path: PathBuf,
    result: Result<Vec<u8>, String>,
    // Whether it saved a copy, which the window does not follow.
    export: bool,
    // The format and metadata of the saved file, which the window shows when it follows it.
    info: Info,
    after: AfterSave,
}

impl ImageWindow {
    /// Shows whether the picture has unsaved changes in the title, with the size of the edited
    /// picture, and in the close button on macOS.
    fn show_edited(&mut self) {
        let edited = self
            .editing
            .as_ref()
            .is_some_and(|editing| editing.editor.is_modified());
        self.window
            .set_title(title(&self.path, Some(&self.view.media()), edited));
        #[cfg(target_os = "macos")]
        self.view.set_edited(edited);
    }
}

impl App {
    /// Runs an edit action in the active window, returns whether it is one.
    pub(crate) fn edit_action(&mut self, action: &str) -> bool {
        let edit = action.starts_with("edit.")
            || action.starts_with("image.")
            || action.starts_with("filter.")
            || action.starts_with("tool.")
            || action.starts_with("select.")
            || matches!(action, "file.save" | "file.save_as");
        if !edit {
            return false;
        }
        let Some(id) = self
            .active_window()
            .filter(|window| !window.is_loading())
            .map(|window| window.window.id())
        else {
            return true;
        };
        // Edits wait until the one being applied or saved is done.
        if self.is_busy(id) {
            return true;
        }
        match action {
            "edit.toggle" => {
                if self.editing(id).is_some() {
                    self.finish_editing(id, AfterSave::FinishEditing);
                } else {
                    self.start_editing(id);
                }
            }
            "edit.undo" | "edit.redo" => self.undo(id, action == "edit.redo"),
            "edit.copy" => self.copy(id),
            "tool.apply" => self.apply_tool(id),
            // Escape closes the open tool, or else edits the whole image again.
            "tool.cancel" => {
                if !self.cancel_tool(id) {
                    self.set_selecting(id, None);
                }
            }
            // All of the image is selected without a selection.
            "select.all" | "select.whole" | "select.none" => self.set_selecting(id, None),
            "select.invert" => {
                if let Some(window) = self.window(id)
                    && window.editing.is_some()
                {
                    window.view.invert_selection();
                }
            }
            "file.save" => _ = self.save(id, false, AfterSave::Nothing),
            // Saving a copy of a viewed image edits it for as long as that takes, vectors and
            // animations are saved as bitmaps.
            "file.save_as" => {
                let viewing = self.editing(id).is_none();
                if viewing && !self.is_editable(id) {
                    self.export_copy(id);
                } else if viewing {
                    if self.start_editing(id) && !self.save(id, true, AfterSave::FinishEditing) {
                        self.finish_editing(id, AfterSave::Nothing);
                    }
                } else {
                    self.save(id, true, AfterSave::Nothing);
                }
            }
            // The menu follows a drawn selection after every event.
            "select.changed" => {}
            _ => {
                if self.editing(id).is_none() && !self.start_editing(id) {
                    return true;
                }
                self.cancel_tool(id);
                self.run(id, action);
            }
        }
        true
    }

    // Runs an action that changes the picture or opens a tool.
    fn run(&mut self, id: WindowId, action: &str) {
        let selection = self.window(id).and_then(|window| window.view.selection());
        let flip = match action {
            "image.flip_horizontal" => Some(true),
            "image.flip_vertical" => Some(false),
            _ => None,
        };
        if let (Some(horizontal), Some(selection)) =
            (flip, selection.filter(|selection| !selection.inverted))
        {
            // Flips of a selection mirror it in place, where it stays selected.
            self.change(id, false, |editor| {
                let flipped = selection.flip(editor.picture(), horizontal);
                editor.apply(flipped);
            });
        } else if let Some((_, orientation)) = ORIENTATIONS.iter().find(|(name, _)| *name == action)
        {
            self.change(id, true, |editor| editor.orient(*orientation));
        } else if let Some(shape) = match action {
            "select.rectangle" => Some(Shape::Rectangle),
            "select.ellipse" => Some(Shape::Ellipse),
            _ => None,
        } {
            self.set_selecting(id, Some(shape));
        } else if action == "image.crop" {
            // Cropping a selection keeps the rectangle around it right away, an ellipse with a
            // transparent background.
            match selection.filter(|selection| !selection.inverted) {
                Some(selection) => {
                    let Some(base) = self
                        .editing(id)
                        .map(|editing| editing.editor.shared_picture())
                    else {
                        return;
                    };
                    let step = Step {
                        tool: Tool::Crop,
                        values: Tool::Crop.initial_values(&base, false),
                        selection: Some(selection),
                        crop: Some(selection.bounds(base.width, base.height)),
                    };
                    self.apply_in_background(id, base, step, false);
                }
                None => self.open_tool(id, Tool::Crop, action),
            }
        } else if action == "image.resize" {
            self.open_tool(id, Tool::Resize, action);
        } else if let Some((_, filter)) = FILTERS.iter().find(|(name, _)| *name == action) {
            if filter.parameters().is_empty() {
                let Some(base) = self
                    .editing(id)
                    .map(|editing| editing.editor.shared_picture())
                else {
                    return;
                };
                let step = Step {
                    tool: Tool::Filter(*filter),
                    values: Vec::new(),
                    selection,
                    crop: None,
                };
                self.apply_in_background(id, base, step, false);
            } else {
                self.open_tool(id, Tool::Filter(*filter), action);
            }
        }
    }

    fn editing(&mut self, id: WindowId) -> Option<&mut Editing> {
        self.window(id)?.editing.as_mut()
    }

    // Whether an edit is being applied or the image saved, which holds off other changes.
    fn is_busy(&mut self, id: WindowId) -> bool {
        self.window(id)
            .is_some_and(|window| window.busy || window.is_modal())
    }

    // Shows that the window is busy or done.
    fn set_busy(&mut self, id: WindowId, busy: bool) {
        if let Some(window) = self.window(id) {
            window.busy = busy;
            window.view.set_busy(busy);
        }
    }

    // Whether the window shows a still bitmap, vectors and animations are only saved as
    // bitmaps.
    fn is_editable(&mut self, id: WindowId) -> bool {
        self.window(id)
            .is_some_and(|window| window.view.media().is_still())
    }

    // Edits the shown frame of a bitmap, with the bytes of its file to keep when possible.
    fn start_editing(&mut self, id: WindowId) -> bool {
        if !self.is_editable(id) {
            return false;
        }
        let Some(window) = self.window(id) else {
            return false;
        };
        if window.is_loading() {
            return false;
        }
        // A page of a file with several is not the file, its bytes cannot be kept.
        let source = window
            .pages
            .is_empty()
            .then(|| std::fs::read(&window.path).ok())
            .flatten();
        let Some(editor) = Editor::new(window.view.media(), 0, source) else {
            return false;
        };
        window.editing = Some(Editing {
            editor,
            tool: None,
            proxy: None,
            preview: 0,
            previewing: false,
            stale: false,
            jpeg_quality: DEFAULT_JPEG_QUALITY,
        });
        window.view.set_editing(true);
        window.view.set_selecting(None);
        true
    }

    // Draws selections of a shape, or edits the whole image without a selection.
    fn set_selecting(&mut self, id: WindowId, shape: Option<Shape>) {
        if let Some(window) = self.window(id)
            && window.editing.is_some()
        {
            window.view.set_selecting(shape);
        }
    }

    // Puts the selection, or the whole picture, on the clipboard.
    fn copy(&mut self, id: WindowId) {
        let Some(window) = self.window(id) else {
            return;
        };
        let picture = match &window.editing {
            Some(editing) => match window.view.selection() {
                Some(selection) => selection.cut(editing.editor.picture()),
                None => editing.editor.picture().clone(),
            },
            None => match Picture::from_media(&window.view.media(), 0) {
                Some(picture) => picture,
                None => return,
            },
        };
        if let Ok(png) = picture.encode_for(Path::new("copy.png"), DEFAULT_JPEG_QUALITY) {
            window.view.copy_png(&png);
        }
    }

    /// Leaves edit mode, asking to save changes first. Returns whether it was left, which is
    /// not while the window is busy or when the changes are kept. When they are saved first,
    /// `after` continues once the save succeeded.
    pub(crate) fn finish_editing(&mut self, id: WindowId, after: AfterSave) -> bool {
        if self.is_busy(id) {
            return false;
        }
        self.cancel_tool(id);
        let modified = self
            .editing(id)
            .is_some_and(|editing| editing.editor.is_modified());
        if modified && !self.drop_changes(id, after) {
            return false;
        }
        self.leave_editing(id, modified);
        true
    }

    // Discard returns to the saved image without asking again after an asynchronous answer.
    pub(super) fn leave_editing(&mut self, id: WindowId, modified: bool) {
        let Some(window) = self.window(id) else {
            return;
        };
        let Some(editing) = window.editing.take() else {
            return;
        };
        if modified {
            window
                .view
                .set_media(editing.editor.original().clone(), true);
        }
        window.show_edited();
        window.view.set_selecting(None);
        window.view.set_editing(false);
        // The size may have changed.
        window.show_info();
    }

    // Asks whether to save the changes, returns whether they are dropped. Saving continues
    // with `after` once it succeeded.
    #[cfg(not(windows))]
    fn drop_changes(&mut self, id: WindowId, after: AfterSave) -> bool {
        let Some(window) = self.window(id) else {
            return true;
        };
        let result = MessageDialog::new()
            .parent(&window.window)
            .level(MessageLevel::Warning)
            .title(format!(
                "Do you want to save the changes made to {}?",
                file_name(&window.path)
            ))
            .description("Your changes will be lost if you don't save them.")
            .buttons(MessageButtons::YesNoCancelCustom(
                "Save".into(),
                "Don't Save".into(),
                "Cancel".into(),
            ))
            .show();
        match result {
            MessageDialogResult::Custom(button) if button == "Save" => {
                self.save(id, false, after);
                false
            }
            MessageDialogResult::Custom(button) => button == "Don't Save",
            _ => false,
        }
    }

    #[cfg(windows)]
    fn drop_changes(&mut self, id: WindowId, after: AfterSave) -> bool {
        self.ask_save_changes(id, after);
        false
    }

    // Changes the picture and shows it, fitted after its size changes.
    fn change(&mut self, id: WindowId, zoom_to_fit: bool, change: impl FnOnce(&mut Editor)) {
        let Some(window) = self.window(id) else {
            return;
        };
        let Some(editing) = &mut window.editing else {
            return;
        };
        change(&mut editing.editor);
        window
            .view
            .set_media(editing.editor.picture().to_media(), zoom_to_fit);
        window.show_edited();
        // A picture of another size has no selection.
        if zoom_to_fit {
            window.view.set_selecting(None);
        }
    }

    fn undo(&mut self, id: WindowId, redo: bool) {
        // Undo first closes an open tool, without changing the picture.
        if self.cancel_tool(id) {
            return;
        }
        self.change(id, true, |editor| {
            if redo {
                editor.redo();
            } else {
                editor.undo();
            }
        });
    }

    /// Enables the menu items that work in the mode of the active window: viewing, editing or
    /// busy applying an edit or saving, and names Edit Image after the mode.
    pub(crate) fn update_menu(&mut self) {
        let modal = self.active_window().is_some_and(|window| window.is_modal());
        let window = self
            .active_window()
            .filter(|window| !window.is_loading() && !window.is_modal());
        let shown = window.is_some();
        let idle = shown && window.as_ref().is_some_and(|window| !window.busy);
        let editing = window.as_ref().and_then(|window| window.editing.as_ref());
        let edit = idle && editing.is_some();
        let editable = idle
            && window
                .as_ref()
                .is_some_and(|window| window.view.media().is_still());
        let selected = edit
            && window
                .as_ref()
                .is_some_and(|window| window.view.selection().is_some());
        let undo =
            editing.is_some_and(|editing| editing.tool.is_some() || editing.editor.can_undo());
        let redo = editing.is_some_and(|editing| editing.editor.can_redo());
        let modified = editing.is_some_and(|editing| editing.editor.is_modified());
        // GTK gives accelerators the keys before a focused field of a tool, so the ones that
        // edit text only work in the image while no tool is open.
        let keys =
            cfg!(target_os = "macos") || editing.is_none_or(|editing| editing.tool.is_none());
        let states = [
            ("file.open", !modal),
            ("file.save", edit && modified),
            ("file.save_as", idle),
            #[cfg(target_os = "macos")]
            ("file.print", shown),
            ("edit.toggle", editable),
            ("edit.undo", edit && undo && keys),
            ("edit.redo", edit && redo && keys),
            ("edit.copy", idle && keys),
            ("select.all", edit && keys),
            ("select.none", selected),
            ("select.invert", selected),
            ("select.whole", edit),
            ("select.rectangle", edit),
            ("select.ellipse", edit),
            ("image.crop", edit),
            ("image.resize", edit),
            ("view.previous", idle && keys),
            ("view.next", idle && keys),
            ("view.zoom_in", shown),
            ("view.zoom_out", shown),
            ("view.actual_size", shown),
            ("view.zoom_to_fit", shown),
        ];
        let tools = ORIENTATIONS
            .iter()
            .map(|(action, _)| *action)
            .chain(FILTERS.iter().map(|(action, _)| *action))
            .map(|action| (action, edit));
        for (action, enabled) in states.into_iter().chain(tools) {
            bwindow::set_menu_item_enabled(action, enabled);
        }
        bwindow::set_menu_item_title(
            "edit.toggle",
            if editing.is_some() {
                "Done Editing"
            } else {
                "Edit Image"
            },
        );
    }

    // Opens the popover of a tool, set up like the last change when it was made by the tool.
    fn open_tool(&mut self, id: WindowId, tool: Tool, action: &str) {
        let Some(window) = self.window(id) else {
            return;
        };
        let Some(editing) = &mut window.editing else {
            return;
        };
        // The last change is set up again when it was made by the tool, of the same selection
        // for a filter, otherwise the tool adds a change.
        let selection = window.view.selection();
        let last = editing
            .editor
            .last_step(tool)
            .filter(|(_, step)| !matches!(tool, Tool::Filter(_)) || step.selection == selection)
            .map(|(before, step)| (before, step.clone()));
        let open = match &last {
            Some((before, step)) => OpenTool {
                tool,
                values: step.values.clone(),
                base: Arc::clone(before),
                amend: true,
                selection: step.selection,
            },
            None => OpenTool {
                tool,
                values: tool.initial_values(editing.editor.picture(), selection.is_some()),
                base: editing.editor.shared_picture(),
                amend: false,
                selection,
            },
        };
        if open.amend {
            // Show the picture before the change, which a crop also shows uncropped, without
            // a selection.
            window.view.set_media(open.base.to_media(), tool.resizes());
            if tool.resizes() {
                window.view.set_selecting(None);
            }
        }
        let selected = open.selection.is_some();
        let controls = tool.controls(selected);
        // Reset goes back to the neutral values, also when the last change is set up again.
        let defaults = tool.initial_values(&open.base, selected);
        window
            .view
            .show_tool(id, tool, &controls, &open.values, &defaults, action);
        // Previews of another tool that are still being made are not shown.
        editing.preview += 1;
        if tool == Tool::Crop {
            window
                .view
                .set_crop_aspect(crop_aspect(open.values[0] as usize, &open.base));
            if let Some(crop) = last.and_then(|(_, step)| step.crop) {
                window.view.set_crop_rect(crop);
            }
        }
        editing.tool = Some(open);
        self.preview(id);
    }

    /// Follows a change of the control at `index` of the open tool of a window.
    pub(crate) fn tool_changed(&mut self, id: WindowId, index: usize, mut values: Vec<f32>) {
        let Some(window) = self.window(id) else {
            return;
        };
        let Some(editing) = &mut window.editing else {
            return;
        };
        let Some(open) = &mut editing.tool else {
            return;
        };
        let given = values.clone();
        open.tool.changed(&mut values, index, &open.base);
        if values != given {
            window.view.set_tool_values(&values);
        }
        if open.tool == Tool::Crop {
            window
                .view
                .set_crop_aspect(crop_aspect(values[0] as usize, &open.base));
        }
        open.values = values;
        self.preview(id);
    }

    // Previews the open tool on a worker thread, or after the preview that is being made.
    fn preview(&mut self, id: WindowId) {
        let proxy_sender = Arc::clone(&self.proxy);
        let Some(window) = self.window(id) else {
            return;
        };
        let longest = window.view.longest_pixels().max(1);
        let Some(editing) = &mut window.editing else {
            return;
        };
        let Some(open) = editing.tool.clone().filter(|open| open.tool.previews()) else {
            return;
        };
        if editing.previewing {
            editing.stale = true;
            return;
        }
        editing.previewing = true;
        editing.preview += 1;
        let generation = editing.preview;
        // The copy of the picture is made once per tool, on the worker.
        let proxy = editing
            .proxy
            .as_ref()
            .filter(|proxy| Arc::ptr_eq(&proxy.base, &open.base))
            .map(|proxy| (Arc::clone(&proxy.picture), proxy.scale));
        std::thread::spawn(move || {
            let (proxy, scale) = proxy.unwrap_or_else(|| {
                let (picture, scale) = open.base.proxy(longest);
                (Arc::new(picture), scale)
            });
            let media = open
                .tool
                .preview(&proxy, scale, &open.values, open.selection.as_ref())
                .to_media();
            _ = proxy_sender
                .send_user_event(AppEvent::Previewed(id, generation, media, proxy, scale));
        });
    }

    /// Shows a preview, when its tool is still open, and starts the next one.
    pub(crate) fn previewed(
        &mut self,
        id: WindowId,
        generation: u64,
        media: Media,
        proxy: Arc<Picture>,
        scale: f64,
    ) {
        let Some(window) = self.window(id) else {
            return;
        };
        let Some(editing) = &mut window.editing else {
            return;
        };
        editing.previewing = false;
        // Only the latest preview is of the open tool and its picture.
        if let Some(open) = &editing.tool
            && editing.preview == generation
        {
            editing.proxy = Some(Proxy {
                base: Arc::clone(&open.base),
                picture: proxy,
                scale,
            });
            window.view.set_preview(Some(media));
        }
        if std::mem::take(&mut editing.stale) {
            self.preview(id);
        }
    }

    // Closes the open tool, returns the picture it changes, the step it makes and whether it
    // amends the last one.
    fn close_tool(&mut self, id: WindowId) -> Option<(Arc<Picture>, Step, bool)> {
        let window = self.window(id)?;
        let open = window.editing.as_mut()?.tool.take()?;
        let crop = (open.tool == Tool::Crop).then(|| window.view.crop_rect());
        window.view.close_tool();
        let step = Step {
            tool: open.tool,
            values: open.values,
            selection: open.selection,
            crop,
        };
        Some((open.base, step, open.amend))
    }

    // Closes the open tool and applies it in the background.
    fn apply_tool(&mut self, id: WindowId) {
        if let Some((base, step, amend)) = self.close_tool(id) {
            self.apply_in_background(id, base, step, amend);
        }
    }

    // Applies a step to a picture on a worker thread, showing that it is busy.
    fn apply_in_background(&mut self, id: WindowId, base: Arc<Picture>, step: Step, amend: bool) {
        let proxy = Arc::clone(&self.proxy);
        self.set_busy(id, true);
        std::thread::spawn(move || {
            let picture = step.apply(&base);
            _ = proxy.send_user_event(AppEvent::Applied(id, picture, step, amend));
        });
    }

    /// Shows an edit that was applied in the background.
    pub(crate) fn applied(&mut self, id: WindowId, picture: Picture, step: Step, amend: bool) {
        self.set_busy(id, false);
        // A filter only changes the colors, so the zoom and selection stay.
        let resizes = step.tool.resizes();
        self.change(id, resizes, |editor| {
            editor.apply_step(picture, step, amend)
        });
    }

    // Closes the open tool without changing the picture, returns whether there was one.
    fn cancel_tool(&mut self, id: WindowId) -> bool {
        let Some(window) = self.window(id) else {
            return false;
        };
        let Some(editing) = window.editing.as_mut() else {
            return false;
        };
        let Some(open) = editing.tool.take() else {
            return false;
        };
        // Previews that are still being made are not shown.
        editing.preview += 1;
        window.view.close_tool();
        if open.amend {
            self.change(id, open.tool.resizes(), |_| {});
        } else {
            window.view.set_preview(None);
        }
        true
    }

    // Saves the picture to its file, or asks for a new one, in the background, then continues
    // with `after`. Unchanged pictures are not written again. Returns whether it continues,
    // which is not when no file is chosen.
    pub(super) fn save(&mut self, id: WindowId, save_as: bool, after: AfterSave) -> bool {
        // An open tool is applied first, here and now.
        if let Some((base, step, amend)) = self.close_tool(id) {
            let picture = step.apply(&base);
            self.change(id, step.tool.resizes(), |editor| {
                editor.apply_step(picture, step, amend)
            });
        }
        let Some(window) = self.window(id) else {
            return false;
        };
        let Some(editing) = &mut window.editing else {
            return false;
        };
        if !save_as && !editing.editor.is_modified() {
            self.continue_after_save(id, after);
            return true;
        }
        let path = if save_as || !can_save(&window.path) {
            // Formats that cannot be saved are saved as PNG.
            let name = if can_save(&window.path) {
                file_name(&window.path)
            } else {
                let stem = window.path.file_stem().map_or_else(
                    || "Untitled".into(),
                    |stem| stem.to_string_lossy().into_owned(),
                );
                format!("{stem}.{DEFAULT_SAVE_EXTENSION}")
            };
            let request = SaveRequest {
                directory: window.path.parent(),
                name: &name,
                quality: editing.jpeg_quality,
                formats: SAVE_FORMATS,
                size: None,
            };
            let Some(choice) = ask_save(&window.window, &request) else {
                return false;
            };
            editing.jpeg_quality = choice.quality;
            choice.path
        } else {
            window.path.clone()
        };
        let (encoding, quality) = (editing.editor.encoding(), editing.jpeg_quality);
        self.save_in_background(id, path, false, after, move |path| {
            encoding.encode(path, quality)
        });
        true
    }

    // Saves a copy of an image that cannot be edited as a bitmap in the background: a vector
    // drawn at a chosen size, or the first frame of an animation. Formats without transparency
    // are offered for opaque images.
    fn export_copy(&mut self, id: WindowId) {
        let Some(window) = self.window(id) else {
            return;
        };
        let media = window.view.media();
        let Some(natural) = Picture::render(&media, 1.0) else {
            return;
        };
        let opaque = natural.is_opaque();
        let stem = window.path.file_stem().map_or_else(
            || "Untitled".into(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let formats: Vec<(&str, &str)> = SAVE_FORMATS
            .iter()
            .copied()
            .filter(|(_, extension)| opaque || !is_jpeg(Path::new(&format!("a.{extension}"))))
            .collect();
        let request = SaveRequest {
            directory: window.path.parent(),
            name: &format!("{stem}.{DEFAULT_SAVE_EXTENSION}"),
            quality: DEFAULT_JPEG_QUALITY,
            formats: &formats,
            size: matches!(media, Media::Vector(_)).then(|| media.size()),
        };
        let Some(choice) = ask_save(&window.window, &request) else {
            return;
        };
        let (scale, quality) = (choice.scale, choice.quality);
        self.save_in_background(id, choice.path, true, AfterSave::Nothing, move |path| {
            let picture = if scale == 1.0 {
                Some(natural)
            } else {
                Picture::render(&media, scale)
            };
            picture
                .ok_or_else(|| "The image could not be drawn".to_owned())
                .and_then(|picture| picture.encode_for(path, quality))
        });
    }

    // Encodes and writes a file on a worker thread, showing that the window is busy.
    fn save_in_background(
        &mut self,
        id: WindowId,
        path: PathBuf,
        export: bool,
        after: AfterSave,
        encode: impl FnOnce(&Path) -> Result<Vec<u8>, String> + Send + 'static,
    ) {
        let proxy = Arc::clone(&self.proxy);
        self.set_busy(id, true);
        std::thread::spawn(move || {
            let result =
                encode(&path).and_then(|bytes| write_atomically(&path, &bytes).map(|()| bytes));
            // The saved file can have other metadata than the one it replaced.
            let info = result
                .as_ref()
                .ok()
                .filter(|_| !export)
                .and_then(|bytes| decode_pages(bytes).ok())
                .map(|pages| pages.info)
                .unwrap_or_default();
            let saved = Saved {
                path,
                result,
                export,
                info,
                after,
            };
            _ = proxy.send_user_event(AppEvent::Saved(id, saved));
        });
    }

    /// Follows a save that finished in the background, and continues after it succeeded.
    pub(crate) fn saved(&mut self, id: WindowId, saved: Saved) {
        self.set_busy(id, false);
        let Some(window) = self.window(id) else {
            return;
        };
        let bytes = match saved.result {
            Ok(bytes) => bytes,
            Err(error) => {
                #[cfg(windows)]
                self.show_error(
                    id,
                    format!("Could not save {}", file_name(&saved.path)),
                    error,
                    false,
                );
                #[cfg(not(windows))]
                MessageDialog::new()
                    .parent(&window.window)
                    .level(MessageLevel::Error)
                    .title(format!("Could not save {}", file_name(&saved.path)))
                    .description(error)
                    .show();
                return;
            }
        };
        if !saved.export
            && let Some(editing) = &mut window.editing
        {
            editing.editor.saved(bytes);
            window.version = FileVersion::of(&saved.path);
            if saved.path != window.path {
                crate::platforms::note_recent_file(&saved.path);
                window.path = saved.path;
            }
            // The saved file only has the edited page, with the metadata it was saved with.
            window.set_pages(Vec::new(), 0, saved.info);
            window.show_edited();
        }
        self.continue_after_save(id, saved.after);
    }

    pub(super) fn continue_after_save(&mut self, id: WindowId, after: AfterSave) {
        match after {
            AfterSave::Nothing => {}
            AfterSave::FinishEditing => _ = self.finish_editing(id, AfterSave::Nothing),
            AfterSave::Close => {
                if self.finish_editing(id, AfterSave::Nothing) {
                    self.close_window(id);
                }
            }
            AfterSave::Show(path) => self.show(id, path),
        }
    }
}

// Writes a file through a temporary file next to it, so it is never half written.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_file_name(format!(".{}.saving", file_name(path)));
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| {
            _ = std::fs::remove_file(&temporary);
            error.to_string()
        })
}
