/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Editing a still image: the picture with its undo history, the tools and their controls,
//! and the filters, shared by the platform editors.

use std::path::Path;
use std::sync::Arc;

pub use image::Orientation;
use image::{Format, copy_jpeg_exif, reorient_jpeg};

pub use self::filter::{Filter, Parameter};
use self::filter::{auto_enhance, straighten};
use self::picture::save_format;
pub use self::picture::{
    DEFAULT_JPEG_QUALITY, DEFAULT_SAVE_EXTENSION, Picture, SAVE_FORMATS, can_save, is_jpeg,
};
pub use self::selection::{Selection, Shape};
use crate::Media;

mod filter;
mod picture;
mod selection;

// The most memory the pictures that can be undone and redone take, in bytes. The last change
// can always be undone.
const HISTORY_BYTES: usize = 1 << 30;

/// The scales of their natural size vectors can be saved at as bitmaps.
pub const EXPORT_SCALES: &[f64] = &[0.5, 1.0, 1.5, 2.0, 3.0, 4.0];

/// The aspect ratios a crop can keep.
pub const CROP_ASPECTS: &[&str] = &[
    "Free", "Original", "Square", "4:3", "3:4", "3:2", "2:3", "16:9", "9:16",
];

/// Returns the width divided by height a crop keeps, or `None` for a free crop.
pub fn crop_aspect(index: usize, picture: &Picture) -> Option<f64> {
    match *CROP_ASPECTS.get(index)? {
        "Free" => None,
        "Original" => Some(f64::from(picture.width) / f64::from(picture.height)),
        "Square" => Some(1.0),
        ratio => {
            let (width, height) = ratio.split_once(':')?;
            Some(width.parse::<f64>().ok()? / height.parse::<f64>().ok()?)
        }
    }
}

// The slider that softens the edges of a selection.
const FEATHER: Parameter = Parameter {
    label: "Feather",
    min: 0.0,
    max: 100.0,
    default: 0.0,
    section: "Selection",
    pixels: true,
};
// The slider that turns the picture before it is cropped.
const STRAIGHTEN: Parameter = Parameter {
    label: "Straighten",
    min: -45.0,
    max: 45.0,
    default: 0.0,
    section: "",
    pixels: false,
};

/// A control of a tool popover, which has one number as its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    /// A slider between a range.
    Slider(Parameter),
    /// A whole number field.
    Number {
        /// The name in front of the field.
        label: &'static str,
    },
    /// A choice from a list of names, its value is the index.
    Choice {
        /// The name in front of the list.
        label: &'static str,
        /// The names to choose from.
        options: &'static [&'static str],
    },
    /// A checkbox, its value is 1 when checked.
    Toggle {
        /// The name of the checkbox.
        label: &'static str,
    },
}

/// An edit that is set up in a popover before it is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Keeps a rectangle that is dragged on the image.
    Crop,
    /// Scales to a size.
    Resize,
    /// A filter with sliders, previewed while they move.
    Filter(Filter),
}

impl Tool {
    /// The title of the popover.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Crop => "Crop",
            Self::Resize => "Resize",
            Self::Filter(filter) => filter.title(),
        }
    }

    /// The title of the button that applies the tool.
    pub const fn apply_label(self) -> &'static str {
        match self {
            Self::Crop => "Crop",
            Self::Resize => "Resize",
            Self::Filter(_) => "Apply",
        }
    }

    /// The controls of the popover, a filter of a selection also softens its edges.
    pub fn controls(self, selected: bool) -> Vec<Control> {
        match self {
            Self::Crop => vec![
                Control::Choice {
                    label: "Aspect Ratio",
                    options: CROP_ASPECTS,
                },
                Control::Slider(STRAIGHTEN),
            ],
            Self::Resize => vec![
                Control::Number { label: "Width" },
                Control::Number { label: "Height" },
                Control::Toggle {
                    label: "Scale proportionally",
                },
            ],
            Self::Filter(filter) => filter
                .parameters()
                .iter()
                .copied()
                .chain(selected.then_some(FEATHER))
                .map(Control::Slider)
                .collect(),
        }
    }

    /// The values the controls start with, which their reset goes back to.
    pub fn initial_values(self, picture: &Picture, selected: bool) -> Vec<f32> {
        match self {
            Self::Crop => vec![0.0, STRAIGHTEN.default],
            Self::Resize => vec![picture.width as f32, picture.height as f32, 1.0],
            Self::Filter(filter) => {
                let mut values = filter.defaults();
                if selected {
                    values.push(FEATHER.default);
                }
                values
            }
        }
    }

    /// Follows a change of the control at `index`, like keeping the proportions of a resize.
    pub fn changed(self, values: &mut [f32], index: usize, picture: &Picture) {
        if self != Self::Resize || values.len() < 3 || values[2] == 0.0 {
            return;
        }
        let ratio = picture.width as f32 / picture.height as f32;
        match index {
            0 => values[1] = (values[0] / ratio).round().max(1.0),
            1 | 2 => values[0] = (values[1] * ratio).round().max(1.0),
            _ => {}
        }
    }

    /// Whether the image shows the result while the controls change.
    pub const fn previews(self) -> bool {
        !matches!(self, Self::Resize)
    }

    /// Whether the tool changes the size of the picture, which is fitted to the window again.
    pub const fn resizes(self) -> bool {
        matches!(self, Self::Crop | Self::Resize)
    }

    /// Previews the tool on a smaller copy of the picture shown at `scale`, with the lengths
    /// in pixels of the values and selection scaled along, so it looks like the result.
    pub fn preview(
        self,
        proxy: &Picture,
        scale: f64,
        values: &[f32],
        selection: Option<&Selection>,
    ) -> Picture {
        let parameters: Vec<Parameter> = self
            .controls(selection.is_some())
            .into_iter()
            .filter_map(|control| match control {
                Control::Slider(parameter) => Some(parameter),
                _ => None,
            })
            .collect();
        let values: Vec<f32> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                if matches!(self, Self::Filter(_))
                    && parameters
                        .get(index)
                        .is_some_and(|parameter| parameter.pixels)
                {
                    value * scale as f32
                } else {
                    *value
                }
            })
            .collect();
        let selection = selection.map(|selection| Selection {
            edges: selection.edges.map(|edge| edge * scale),
            ..*selection
        });
        self.apply(proxy, &values, None, selection.as_ref())
    }

    /// Returns the edited picture. A crop keeps the `crop` rectangle in pixels, and cuts out an
    /// ellipse selection, or only turns the picture to preview it without one. A filter only
    /// changes the selection.
    pub fn apply(
        self,
        picture: &Picture,
        values: &[f32],
        crop: Option<(u32, u32, u32, u32)>,
        selection: Option<&Selection>,
    ) -> Picture {
        let value = |index: usize| values.get(index).copied().unwrap_or_default();
        match self {
            Self::Crop => {
                let turned = straighten(picture, value(1));
                match (crop, selection) {
                    // An ellipse is cut out, transparent around it.
                    (Some(_), Some(selection))
                        if selection.shape == Shape::Ellipse && !selection.inverted =>
                    {
                        selection.cut(&turned)
                    }
                    (Some((x, y, width, height)), _) => turned.crop(x, y, width, height),
                    (None, _) => turned,
                }
            }
            Self::Resize => {
                let size = |index: usize| value(index).round().clamp(1.0, 32768.0) as u32;
                picture.resize(size(0), size(1))
            }
            Self::Filter(filter) => {
                let filtered = match selection {
                    // The tones of a selection are measured inside it.
                    Some(selection) if filter == Filter::AutoEnhance && !selection.inverted => {
                        let (x, y, width, height) = selection.bounds(picture.width, picture.height);
                        auto_enhance(picture, &picture.crop(x, y, width, height))
                    }
                    _ => filter.apply(picture, values),
                };
                match selection {
                    Some(selection) => {
                        let feather = value(filter.parameters().len());
                        selection.blend(picture, &filtered, f64::from(feather))
                    }
                    None => filtered,
                }
            }
        }
    }
}

/// A tool applied to a picture, which can be set up again from the picture before it.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// The tool.
    pub tool: Tool,
    /// The values of its controls.
    pub values: Vec<f32>,
    /// The selection it changed.
    pub selection: Option<Selection>,
    /// The rectangle a crop kept, in pixels of the turned picture.
    pub crop: Option<(u32, u32, u32, u32)>,
}

impl Step {
    /// The picture with the tool applied.
    pub fn apply(&self, picture: &Picture) -> Picture {
        self.tool
            .apply(picture, &self.values, self.crop, self.selection.as_ref())
    }
}

// A version of the edited picture.
struct State {
    picture: Arc<Picture>,
    // How the picture is turned from the file, while it has no other changes.
    orientation: Option<Orientation>,
    // The tool that made this version.
    step: Option<Step>,
}

/// A version of an edited picture to save, with the file it was opened from.
pub struct Encoding {
    picture: Arc<Picture>,
    source: Option<Arc<[u8]>>,
    orientation: Option<Orientation>,
}

impl Encoding {
    /// Encodes the picture for a file name, JPEG with a quality from 1 to 100. An unchanged
    /// picture keeps the file as it is and a turned JPEG only changes its orientation, so
    /// neither loses quality. An edited photo keeps the metadata of its JPEG file.
    pub fn encode(&self, path: &Path, jpeg_quality: u8) -> Result<Vec<u8>, String> {
        let source = self.source.as_deref();
        let source_format = source.and_then(Format::from_signature);
        if let (Some(source), Some(orientation)) = (source, self.orientation) {
            let kept = source_format.filter(|format| Some(*format) == save_format(path));
            if orientation == Orientation::Normal && kept.is_some() {
                return Ok(source.to_vec());
            }
            if kept == Some(Format::Jpeg)
                && let Some(bytes) = reorient_jpeg(source, orientation)
            {
                return Ok(bytes);
            }
        }
        let picture = &self.picture;
        let bytes = picture.encode_for(path, jpeg_quality)?;
        Ok(match source {
            Some(source)
                if source_format == Some(Format::Jpeg)
                    && save_format(path) == Some(Format::Jpeg) =>
            {
                copy_jpeg_exif(source, bytes, picture.width, picture.height)
            }
            _ => bytes,
        })
    }
}

/// A picture being edited with the changes that can be undone and redone.
pub struct Editor {
    original: Media,
    // The bytes of the file, which turned or unchanged pictures keep.
    source: Option<Arc<[u8]>>,
    state: State,
    undo: Vec<State>,
    redo: Vec<State>,
    // How many changes could be undone when the picture was last saved, `None` when that
    // version is gone.
    saved: Option<usize>,
}

impl Editor {
    /// Edits a frame of media decoded from `source`, the media is kept to show again when the
    /// edits are dropped.
    pub fn new(original: Media, frame: usize, source: Option<Vec<u8>>) -> Option<Self> {
        Some(Self {
            state: State {
                picture: Arc::new(Picture::from_media(&original, frame)?),
                orientation: Some(Orientation::Normal),
                step: None,
            },
            original,
            source: source.map(Arc::from),
            undo: Vec::new(),
            redo: Vec::new(),
            saved: Some(0),
        })
    }

    /// The media before editing.
    pub const fn original(&self) -> &Media {
        &self.original
    }

    /// The edited picture.
    pub fn picture(&self) -> &Picture {
        &self.state.picture
    }

    /// The edited picture, to share with other threads.
    pub fn shared_picture(&self) -> Arc<Picture> {
        Arc::clone(&self.state.picture)
    }

    /// The last change when it was made by `tool`, with the picture before it, so the tool
    /// can be set up again instead of being applied on top of itself.
    pub fn last_step(&self, tool: Tool) -> Option<(Arc<Picture>, &Step)> {
        let step = self.state.step.as_ref().filter(|step| step.tool == tool)?;
        Some((Arc::clone(&self.undo.last()?.picture), step))
    }

    /// Replaces the picture with an edited one, which can be undone.
    pub fn apply(&mut self, picture: Picture) {
        self.push(State {
            picture: Arc::new(picture),
            orientation: None,
            step: None,
        });
    }

    /// Replaces the picture with the result of a tool, which can be undone. An amended step
    /// replaces the last change, which was made by the same tool, from the same picture.
    pub fn apply_step(&mut self, picture: Picture, step: Step, amend: bool) {
        let amend = amend && self.last_step(step.tool).is_some();
        let state = State {
            picture: Arc::new(picture),
            orientation: None,
            step: Some(step),
        };
        if amend {
            // The saved version is replaced, unless it is the one before.
            if self.saved >= Some(self.undo.len()) {
                self.saved = None;
            }
            self.state = state;
            self.redo.clear();
        } else {
            self.push(state);
        }
    }

    /// Turns or mirrors the picture, which can be undone.
    pub fn orient(&mut self, orientation: Orientation) {
        self.push(State {
            picture: Arc::new(self.state.picture.orient(orientation)),
            orientation: self
                .state
                .orientation
                .map(|current| current.then(orientation)),
            step: None,
        });
    }

    fn push(&mut self, state: State) {
        if self.saved > Some(self.undo.len()) {
            self.saved = None;
        }
        self.undo.push(std::mem::replace(&mut self.state, state));
        self.redo.clear();
        let bytes = |state: &State| state.picture.pixels.len();
        let mut total: usize = self.undo.iter().map(bytes).sum();
        while self.undo.len() > 1 && total > HISTORY_BYTES {
            total -= bytes(&self.undo.remove(0));
            self.saved = self.saved.and_then(|saved| saved.checked_sub(1));
        }
    }

    /// Goes back to the picture before the last change, returns whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(state) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(&mut self.state, state));
        true
    }

    /// Applies the last undone change again, returns whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(state) = self.redo.pop() else {
            return false;
        };
        self.undo.push(std::mem::replace(&mut self.state, state));
        true
    }

    /// Whether there is a change to undo.
    pub const fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is an undone change to redo.
    pub const fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Whether the picture differs from the saved file.
    pub fn is_modified(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    /// What saving encodes, which another thread can encode while the editor waits.
    pub fn encoding(&self) -> Encoding {
        Encoding {
            picture: Arc::clone(&self.state.picture),
            source: self.source.clone(),
            orientation: self.state.orientation,
        }
    }

    /// Encodes the picture for a file name, like `Encoding::encode`.
    pub fn encode_for(&self, path: &Path, jpeg_quality: u8) -> Result<Vec<u8>, String> {
        self.encoding().encode(path, jpeg_quality)
    }

    /// Remembers that the picture is saved as `bytes`, which later turns start from.
    pub fn saved(&mut self, bytes: Vec<u8>) {
        self.saved = Some(self.undo.len());
        self.source = Some(Arc::from(bytes));
        self.original = self.state.picture.to_media();
        // The other versions are no longer turns of the file.
        for state in self.undo.iter_mut().chain(&mut self.redo) {
            state.orientation = None;
        }
        self.state.orientation = Some(Orientation::Normal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The compressed pixels of a JPEG, from its start of scan.
    fn scan(bytes: &[u8]) -> &[u8] {
        let start = bytes
            .windows(2)
            .position(|marker| marker == [0xff, 0xda])
            .expect("start of scan");
        &bytes[start..]
    }

    fn editor() -> Editor {
        let picture = Picture::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        Editor::new(picture.to_media(), 0, None).expect("editor")
    }

    #[test]
    fn changes_undo_and_redo() {
        let mut editor = editor();
        let original = editor.picture().clone();
        assert!(!editor.is_modified() && !editor.can_undo());
        editor.orient(Orientation::Rotate270);
        assert!(editor.is_modified() && editor.can_undo() && !editor.can_redo());
        assert!(editor.undo());
        assert_eq!(editor.picture(), &original);
        assert!(!editor.is_modified());
        assert!(!editor.undo());
        assert!(editor.redo());
        assert_eq!(editor.picture().width, 1);
        editor.saved(Vec::new());
        assert!(!editor.is_modified());
        assert!(editor.undo());
        editor.apply(original.flip_horizontal());
        assert!(!editor.redo());
        assert!(editor.is_modified());
    }

    #[test]
    fn discarding_after_a_save_returns_to_the_saved_picture() {
        let mut editor = editor();
        editor.orient(Orientation::Rotate90);
        let saved = editor.picture().clone();
        editor.saved(Vec::new());
        editor.orient(Orientation::Rotate90);
        assert_ne!(editor.picture(), &saved);
        assert_eq!(Picture::from_media(editor.original(), 0), Some(saved));
    }

    #[test]
    fn unchanged_and_turned_files_are_not_encoded_again() {
        let picture = Picture::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let png = picture
            .encode_for(Path::new("a.png"), DEFAULT_JPEG_QUALITY)
            .expect("png");
        let jpeg = picture
            .encode_for(Path::new("a.jpg"), DEFAULT_JPEG_QUALITY)
            .expect("jpeg");
        let mut editor = Editor::new(picture.to_media(), 0, Some(png.clone())).expect("editor");
        assert_eq!(
            editor.encode_for(Path::new("b.png"), DEFAULT_JPEG_QUALITY),
            Ok(png.clone())
        );
        assert_ne!(
            editor.encode_for(Path::new("b.qoi"), DEFAULT_JPEG_QUALITY),
            Ok(png.clone())
        );
        editor.orient(Orientation::Rotate270);
        assert_ne!(
            editor.encode_for(Path::new("b.png"), DEFAULT_JPEG_QUALITY),
            Ok(png)
        );

        let mut editor = Editor::new(picture.to_media(), 0, Some(jpeg.clone())).expect("editor");
        editor.orient(Orientation::Rotate270);
        editor.orient(Orientation::FlipHorizontal);
        let turned = editor
            .encode_for(Path::new("b.jpg"), DEFAULT_JPEG_QUALITY)
            .expect("turned");
        assert_eq!(scan(&turned), scan(&jpeg));
        editor.saved(turned.clone());
        editor.orient(Orientation::Rotate90);
        let back = editor
            .encode_for(Path::new("b.jpg"), DEFAULT_JPEG_QUALITY)
            .expect("turned back");
        assert_eq!(back.len(), turned.len());
        // Undoing past the save encodes the pixels again.
        editor.undo();
        editor.undo();
        let encoded = editor
            .encode_for(Path::new("b.jpg"), DEFAULT_JPEG_QUALITY)
            .expect("encoded");
        assert_ne!(scan(&encoded), scan(&jpeg));
        let filtered = Filter::Invert.apply(editor.picture(), &[]);
        editor.apply(filtered);
        let encoded = editor
            .encode_for(Path::new("b.jpg"), DEFAULT_JPEG_QUALITY)
            .expect("encoded");
        assert_ne!(scan(&encoded), scan(&jpeg));
    }

    #[test]
    fn history_is_limited_by_memory() {
        // Pictures of a quarter of the budget, so four of them can be undone.
        let side = ((HISTORY_BYTES / 4 / 4) as f64).sqrt() as u32;
        let picture = Picture::new(side, side, vec![0; (side * side * 4) as usize]);
        let mut editor = Editor::new(picture.to_media(), 0, None).expect("editor");
        for _ in 0..6 {
            editor.apply(picture.clone());
        }
        let mut undone = 0;
        while editor.undo() {
            undone += 1;
        }
        assert_eq!(undone, 4);
        assert!(editor.is_modified());
    }

    #[test]
    fn last_steps_are_amended() {
        let mut editor = editor();
        let original = editor.picture().clone();
        let tool = Tool::Filter(Filter::Invert);
        assert!(editor.last_step(tool).is_none());
        let step = |values: Vec<f32>| Step {
            tool,
            values,
            selection: None,
            crop: None,
        };
        editor.apply_step(original.flip_horizontal(), step(vec![1.0]), false);
        let (before, last) = editor.last_step(tool).expect("last step");
        assert_eq!(*before, original);
        assert_eq!(last.values, [1.0]);
        assert!(editor.last_step(Tool::Resize).is_none());
        // Amending replaces the step, so one undo goes back to the original.
        editor.apply_step(original.rotate_left(), step(vec![2.0]), true);
        assert_eq!(editor.picture().width, 1);
        assert!(editor.undo());
        assert_eq!(editor.picture(), &original);
        assert!(!editor.undo());
        // An amended saved version is no longer saved.
        editor.redo();
        editor.saved(Vec::new());
        editor.apply_step(original.clone(), step(vec![3.0]), true);
        assert!(editor.is_modified());
    }

    #[test]
    fn re_encoded_photos_keep_their_exif() {
        let picture = Picture::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let jpeg = picture
            .encode_for(Path::new("a.jpg"), DEFAULT_JPEG_QUALITY)
            .expect("jpeg");
        let mut file = jpeg[..2].to_vec();
        file.extend_from_slice(&[0xff, 0xe1, 0, 16]);
        file.extend_from_slice(b"Exif\0\0MM\0\x2a\0\0\0\x08");
        file.extend_from_slice(&jpeg[2..]);
        let mut editor = Editor::new(picture.to_media(), 0, Some(file)).expect("editor");
        editor.apply(Filter::Invert.apply(editor.picture(), &[]));
        let edited = editor
            .encode_for(Path::new("b.jpg"), DEFAULT_JPEG_QUALITY)
            .expect("jpeg");
        assert!(edited.windows(6).any(|part| part == b"Exif\0\0"));
        let png = editor
            .encode_for(Path::new("b.png"), DEFAULT_JPEG_QUALITY)
            .expect("png");
        assert!(!png.windows(6).any(|part| part == b"Exif\0\0"));
    }

    #[test]
    fn resizing_keeps_proportions() {
        let picture = Picture::new(4, 2, vec![0; 32]);
        let mut values = Tool::Resize.initial_values(&picture, false);
        values[0] = 100.0;
        Tool::Resize.changed(&mut values, 0, &picture);
        assert_eq!(values, [100.0, 50.0, 1.0]);
        values[2] = 0.0;
        values[1] = 10.0;
        Tool::Resize.changed(&mut values, 1, &picture);
        assert_eq!(values, [100.0, 10.0, 0.0]);
        let resized = Tool::Resize.apply(&picture, &values, None, None);
        assert_eq!((resized.width, resized.height), (100, 10));
    }

    #[test]
    fn filters_of_selections_are_feathered() {
        let picture = Picture::new(8, 8, vec![255; 256]);
        let tool = Tool::Filter(Filter::Invert);
        assert_eq!(tool.controls(false), []);
        assert_eq!(tool.controls(true), [Control::Slider(FEATHER)]);
        let selection = Selection {
            shape: Shape::Rectangle,
            edges: [0.0, 0.0, 4.0, 8.0],
            inverted: false,
        };
        let values = tool.initial_values(&picture, true);
        let inverted = tool.apply(&picture, &values, None, Some(&selection));
        assert_eq!(inverted.pixel(1, 1), [0, 0, 0, 255]);
        assert_eq!(inverted.pixel(6, 1), [255; 4]);
        let feathered = tool.apply(&picture, &[20.0], None, Some(&selection));
        assert!(feathered.pixel(3, 4)[0] > inverted.pixel(3, 4)[0]);
    }

    #[test]
    fn previews_scale_lengths_in_pixels() {
        let picture = Picture::new(64, 32, vec![255; 64 * 32 * 4]);
        let (proxy, scale) = picture.proxy(16);
        assert_eq!((proxy.width, proxy.height, scale), (16, 8, 0.25));
        assert_eq!(picture.proxy(100).1, 1.0);
        let tool = Tool::Filter(Filter::Pixelate);
        let selection = Selection {
            shape: Shape::Rectangle,
            edges: [0.0, 0.0, 32.0, 32.0],
            inverted: false,
        };
        // Blocks of 8 pixels are 2 pixels on the proxy, only in its left half.
        let mut checkered = proxy.clone();
        for (index, pixel) in checkered.pixels.chunks_mut(4).enumerate() {
            if (index % 16 + index / 16) % 2 == 0 {
                pixel[..3].fill(0);
            }
        }
        let preview = tool.preview(&checkered, scale, &[8.0, 0.0], Some(&selection));
        assert_eq!(preview.pixel(0, 0), preview.pixel(1, 1));
        assert_eq!(preview.pixel(9, 0), checkered.pixel(9, 0));
    }

    #[test]
    fn auto_enhance_measures_the_selection() {
        // A dark left half and a gray right half.
        let pixels = (0..16)
            .flat_map(|index| {
                if index % 4 < 2 {
                    [20, 20, 20, 255]
                } else {
                    [100, 100, 100, 255]
                }
            })
            .collect();
        let picture = Picture::new(4, 4, pixels);
        let selection = Selection {
            shape: Shape::Rectangle,
            edges: [2.0, 0.0, 4.0, 4.0],
            inverted: false,
        };
        let tool = Tool::Filter(Filter::AutoEnhance);
        // The flat gray selection has no tones to stretch, the dark half is not selected.
        let enhanced = tool.apply(&picture, &[0.0], None, Some(&selection));
        assert_eq!(enhanced, picture);
        let whole = tool.apply(&picture, &[], None, None);
        assert_eq!(whole.pixel(3, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn crops_of_ellipses_are_round() {
        let picture = Picture::new(8, 8, vec![255; 256]);
        let selection = Selection {
            shape: Shape::Ellipse,
            edges: [2.0, 2.0, 6.0, 6.0],
            inverted: false,
        };
        let crop = Some(selection.bounds(8, 8));
        let round = Tool::Crop.apply(&picture, &[0.0, 0.0], crop, Some(&selection));
        assert_eq!((round.width, round.height), (4, 4));
        assert!(round.pixel(0, 0)[3] < 128);
        assert_eq!(round.pixel(2, 2)[3], 255);
        let square = Selection {
            shape: Shape::Rectangle,
            ..selection
        };
        let cropped = Tool::Crop.apply(&picture, &[0.0, 0.0], crop, Some(&square));
        assert_eq!(cropped.pixel(0, 0)[3], 255);
    }

    #[test]
    fn crops_straighten_first() {
        let picture = Picture::new(8, 4, vec![255; 128]);
        let preview = Tool::Crop.apply(&picture, &[0.0, 10.0], None, None);
        assert_eq!((preview.width, preview.height), (8, 4));
        let cropped = Tool::Crop.apply(&picture, &[0.0, 10.0], Some((1, 1, 2, 2)), None);
        assert_eq!((cropped.width, cropped.height), (2, 2));
        assert!(Tool::Crop.previews() && Tool::Crop.resizes() && !Tool::Resize.previews());
    }

    #[test]
    fn crop_aspects_follow_the_picture() {
        let picture = Picture::new(4, 2, vec![0; 32]);
        assert_eq!(crop_aspect(0, &picture), None);
        assert_eq!(crop_aspect(1, &picture), Some(2.0));
        assert_eq!(crop_aspect(2, &picture), Some(1.0));
        assert_eq!(crop_aspect(7, &picture), Some(16.0 / 9.0));
        assert_eq!(crop_aspect(CROP_ASPECTS.len(), &picture), None);
    }
}
