/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The popovers that preview adjustments, effects, and straightening live.

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;

use macview_appkit::{Point, Rect, Size, ns_string};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool, Sel};
use objc2::{class, define_class, msg_send, sel};

use crate::Document;
use crate::edit::{Adjustments, Preset, Selection};
use crate::editing::Operation;
use crate::headers::*;
use crate::toolbar::{ADJUST_ITEM, EFFECTS_ITEM, STRAIGHTEN_ITEM, visible_item};

// The width of the label column and of a slider, in points.
const LABEL_WIDTH: f64 = 86.0;
const SLIDER_WIDTH: f64 = 190.0;
const VALUE_WIDTH: f64 = 44.0;
const COLUMN_SPACING: f64 = 8.0;
const GRID_WIDTH: f64 = LABEL_WIDTH + SLIDER_WIDTH + VALUE_WIDTH + 2.0 * COLUMN_SPACING;
// The space around the contents of a popover.
const INSET: f64 = 16.0;
// The NSGridCellPlacement that aligns a column to its trailing edge.
const GRID_CELL_PLACEMENT_TRAILING: i64 = 3;

/// The popovers that preview an edit live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelKind {
    /// Light and color adjustments and filter presets.
    Adjust,
    /// Blur, sharpen and vignette.
    Effects,
    /// Rotating by a small angle and cropping the tilted edges.
    Straighten,
}

impl PanelKind {
    const fn title(self) -> &'static str {
        match self {
            Self::Adjust => "Adjust Light & Color",
            Self::Effects => "Effects",
            Self::Straighten => "Straighten",
        }
    }

    const fn toolbar_item(self) -> &'static str {
        match self {
            Self::Adjust => ADJUST_ITEM,
            Self::Effects => EFFECTS_ITEM,
            Self::Straighten => STRAIGHTEN_ITEM,
        }
    }

    // Returns the slider sections of the popover: a heading and its fields.
    const fn sections(self) -> &'static [(&'static str, &'static [Field])] {
        match self {
            Self::Adjust => &[
                (
                    "Light",
                    &[
                        Field::Exposure,
                        Field::Brightness,
                        Field::Contrast,
                        Field::Highlights,
                        Field::Shadows,
                    ],
                ),
                (
                    "Color",
                    &[
                        Field::Saturation,
                        Field::Vibrance,
                        Field::Temperature,
                        Field::Tint,
                    ],
                ),
            ],
            Self::Effects => &[("", &[Field::Blur, Field::Sharpen, Field::Vignette])],
            Self::Straighten => &[("", &[Field::Angle])],
        }
    }
}

// A value a slider edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Exposure,
    Brightness,
    Contrast,
    Highlights,
    Shadows,
    Saturation,
    Vibrance,
    Temperature,
    Tint,
    Blur,
    Sharpen,
    Vignette,
    Angle,
    Feather,
}

const FIELDS: [Field; 14] = [
    Field::Exposure,
    Field::Brightness,
    Field::Contrast,
    Field::Highlights,
    Field::Shadows,
    Field::Saturation,
    Field::Vibrance,
    Field::Temperature,
    Field::Tint,
    Field::Blur,
    Field::Sharpen,
    Field::Vignette,
    Field::Angle,
    Field::Feather,
];

impl Field {
    const fn label(self) -> &'static str {
        match self {
            Self::Exposure => "Exposure",
            Self::Brightness => "Brightness",
            Self::Contrast => "Contrast",
            Self::Highlights => "Highlights",
            Self::Shadows => "Shadows",
            Self::Saturation => "Saturation",
            Self::Vibrance => "Vibrance",
            Self::Temperature => "Warmth",
            Self::Tint => "Tint",
            Self::Blur => "Blur",
            Self::Sharpen => "Sharpen",
            Self::Vignette => "Vignette",
            Self::Angle => "Angle",
            Self::Feather => "Feather",
        }
    }

    // Returns the range of the slider.
    const fn range(self) -> (f64, f64) {
        match self {
            Self::Blur => (0.0, 50.0),
            Self::Sharpen => (0.0, 1.0),
            Self::Angle => (-45.0, 45.0),
            Self::Feather => (0.0, 100.0),
            _ => (-1.0, 1.0),
        }
    }

    // Formats a value the way the popover shows it next to the slider.
    fn format(self, value: f64) -> String {
        match self {
            Self::Blur | Self::Feather => format!("{} px", value.round()),
            Self::Angle => format!("{value:+.1}\u{b0}"),
            Self::Sharpen => format!("{}", (value * 100.0).round()),
            _ => {
                let percent = (value * 100.0).round();
                if percent == 0.0 {
                    String::from("0")
                } else {
                    format!("{percent:+}")
                }
            }
        }
    }

    const fn tag(self) -> isize {
        let mut index = 0;
        while index < FIELDS.len() {
            if FIELDS[index] as u8 == self as u8 {
                return index as isize;
            }
            index += 1;
        }
        unreachable!()
    }
}

// The values the sliders and checkboxes of a popover edit.
#[derive(Clone, Copy, Default)]
struct PanelState {
    adjustments: Adjustments,
    degrees: f64,
    feather: f64,
}

impl PanelState {
    fn get(&self, field: Field) -> f64 {
        let adjustments = &self.adjustments;
        f64::from(match field {
            Field::Exposure => adjustments.exposure,
            Field::Brightness => adjustments.brightness,
            Field::Contrast => adjustments.contrast,
            Field::Highlights => adjustments.highlights,
            Field::Shadows => adjustments.shadows,
            Field::Saturation => adjustments.saturation,
            Field::Vibrance => adjustments.vibrance,
            Field::Temperature => adjustments.temperature,
            Field::Tint => adjustments.tint,
            Field::Blur => adjustments.blur,
            Field::Sharpen => adjustments.sharpen,
            Field::Vignette => adjustments.vignette,
            Field::Angle => return self.degrees,
            Field::Feather => return self.feather,
        })
    }

    const fn set(&mut self, field: Field, value: f64) {
        let adjustments = &mut self.adjustments;
        let slot = match field {
            Field::Exposure => &mut adjustments.exposure,
            Field::Brightness => &mut adjustments.brightness,
            Field::Contrast => &mut adjustments.contrast,
            Field::Highlights => &mut adjustments.highlights,
            Field::Shadows => &mut adjustments.shadows,
            Field::Saturation => &mut adjustments.saturation,
            Field::Vibrance => &mut adjustments.vibrance,
            Field::Temperature => &mut adjustments.temperature,
            Field::Tint => &mut adjustments.tint,
            Field::Blur => &mut adjustments.blur,
            Field::Sharpen => &mut adjustments.sharpen,
            Field::Vignette => &mut adjustments.vignette,
            Field::Angle => {
                self.degrees = value;
                return;
            }
            Field::Feather => {
                self.feather = value;
                return;
            }
        };
        *slot = value as f32;
    }
}

// A slider of the popover with the label that shows its value.
struct SliderRow {
    field: Field,
    slider: Retained<Object>,
    value: Retained<Object>,
}

// The checkboxes of the adjustments popover, by tag.
const GRAYSCALE_TAG: isize = 0;
const INVERT_TAG: isize = 1;

pub(crate) struct EditPanelIvars {
    kind: PanelKind,
    document: Retained<Object>,
    selection: Option<Selection>,
    state: Cell<PanelState>,
    // The controls that show the state, to update them when a preset or reset changes it.
    sliders: RefCell<Vec<SliderRow>>,
    checkboxes: RefCell<Vec<Retained<Object>>>,
    presets: RefCell<Option<Retained<Object>>>,
    popover: RefCell<Option<Retained<Object>>>,
    // Whether Apply or Cancel ended the preview, so closing does not cancel it again.
    finished: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MacViewEditPanel"]
    #[ivars = EditPanelIvars]
    pub(crate) struct EditPanel;

    impl EditPanel {
        #[unsafe(method(sliderChanged:))]
        fn _slider_changed(&self, slider: *mut Object) {
            self.slider_changed(slider);
        }

        #[unsafe(method(checkboxChanged:))]
        fn _checkbox_changed(&self, checkbox: *mut Object) {
            self.checkbox_changed(checkbox);
        }

        #[unsafe(method(presetChanged:))]
        fn _preset_changed(&self, popup: *mut Object) {
            self.preset_changed(popup);
        }

        #[unsafe(method(resetValues:))]
        fn _reset_values(&self, _: *mut Object) {
            self.set_state(PanelState {
                feather: self.ivars().state.get().feather,
                ..PanelState::default()
            });
            if let Some(presets) = &*self.ivars().presets.borrow() {
                // SAFETY: The pop-up button is live while the popover is.
                unsafe {
                    let _: () = msg_send![&**presets, selectItemAtIndex: 0isize];
                }
            }
        }

        #[unsafe(method(apply:))]
        fn _apply(&self, _: *mut Object) {
            self.finish(true);
        }

        #[unsafe(method(cancel:))]
        fn _cancel(&self, _: *mut Object) {
            self.finish(false);
        }

        #[unsafe(method(popoverDidClose:))]
        fn _popover_did_close(&self, _: *mut Object) {
            // Escape and other ways of closing the popover discard the preview.
            if !self.ivars().finished.replace(true) {
                self.document().cancel_preview();
            }
        }
    }
);

impl EditPanel {
    const fn as_object(&self) -> *mut Object {
        self as *const Self as *mut Object
    }

    fn document(&self) -> &Document {
        // SAFETY: The panel is only made for Document instances, which it retains.
        unsafe { &*Retained::as_ptr(&self.ivars().document).cast::<Document>() }
    }

    // Returns the edit the popover previews with its current values.
    fn operation(&self) -> Operation {
        let state = self.ivars().state.get();
        match self.ivars().kind {
            PanelKind::Straighten => Operation::Straighten {
                degrees: state.degrees,
            },
            PanelKind::Adjust | PanelKind::Effects => Operation::Adjust {
                adjustments: state.adjustments,
                selection: self.ivars().selection.map(|selection| Selection {
                    feather: state.feather,
                    ..selection
                }),
            },
        }
    }

    fn preview(&self) {
        self.document().update_preview(self.operation());
    }

    fn slider_changed(&self, slider: *mut Object) {
        // SAFETY: The sender is one of the sliders of the popover.
        let (tag, value): (isize, f64) =
            unsafe { (msg_send![slider, tag], msg_send![slider, doubleValue]) };
        let Some(&field) = usize::try_from(tag).ok().and_then(|tag| FIELDS.get(tag)) else {
            return;
        };
        let mut state = self.ivars().state.get();
        state.set(field, value);
        self.ivars().state.set(state);
        self.update_value_labels();
        self.preview();
    }

    fn checkbox_changed(&self, checkbox: *mut Object) {
        // SAFETY: The sender is one of the checkboxes of the popover.
        let (tag, on): (isize, isize) =
            unsafe { (msg_send![checkbox, tag], msg_send![checkbox, state]) };
        let mut state = self.ivars().state.get();
        let on = on == NS_CONTROL_STATE_VALUE_ON;
        match tag {
            GRAYSCALE_TAG => state.adjustments.grayscale = on,
            INVERT_TAG => state.adjustments.invert = on,
            _ => return,
        }
        self.ivars().state.set(state);
        self.preview();
    }

    fn preset_changed(&self, popup: *mut Object) {
        // SAFETY: The sender is the preset pop-up button, whose first item is None.
        let index: isize = unsafe { msg_send![popup, indexOfSelectedItem] };
        let adjustments = usize::try_from(index - 1)
            .ok()
            .and_then(|index| Preset::ALL.get(index))
            .map(|preset| preset.adjustments())
            .unwrap_or_default();
        let state = self.ivars().state.get();
        self.set_state(PanelState {
            adjustments,
            ..state
        });
    }

    // Shows `state` in every control and previews it.
    fn set_state(&self, state: PanelState) {
        self.ivars().state.set(state);
        // SAFETY: The controls are live while the popover is.
        unsafe {
            for row in &*self.ivars().sliders.borrow() {
                let _: () = msg_send![&*row.slider, setDoubleValue: state.get(row.field)];
            }
            for checkbox in &*self.ivars().checkboxes.borrow() {
                let tag: isize = msg_send![&**checkbox, tag];
                let on = match tag {
                    GRAYSCALE_TAG => state.adjustments.grayscale,
                    _ => state.adjustments.invert,
                };
                let value = if on {
                    NS_CONTROL_STATE_VALUE_ON
                } else {
                    NS_CONTROL_STATE_VALUE_OFF
                };
                let _: () = msg_send![&**checkbox, setState: value];
            }
        }
        self.update_value_labels();
        self.preview();
    }

    fn update_value_labels(&self) {
        let state = self.ivars().state.get();
        for row in &*self.ivars().sliders.borrow() {
            // SAFETY: The label is live while the popover is.
            unsafe {
                let _: () = msg_send![&*row.value,
                    setStringValue: ns_string(&row.field.format(state.get(row.field)))
                ];
            }
        }
    }

    fn finish(&self, apply: bool) {
        if self.ivars().finished.replace(true) {
            return;
        }
        if apply {
            self.document().commit_preview(self.ivars().kind.title());
        } else {
            self.document().cancel_preview();
        }
        if let Some(popover) = &*self.ivars().popover.borrow() {
            // SAFETY: The popover is live while the panel holds it.
            unsafe {
                let _: () = msg_send![&**popover, performClose: null_mut::<Object>()];
            }
        }
    }

    // Returns an autoreleased label.
    unsafe fn label(text: &str) -> *mut Object {
        // SAFETY: NSTextField returns an autoreleased label.
        unsafe { msg_send![class!(NSTextField), labelWithString: ns_string(text)] }
    }

    // Returns an autoreleased push button that sends `action` to the panel.
    unsafe fn button(&self, title: &str, action: Sel, key: &str) -> *mut Object {
        // SAFETY: NSButton returns an autoreleased button that keeps a weak target.
        unsafe {
            let button: *mut Object = msg_send![class!(NSButton),
                buttonWithTitle: ns_string(title),
                target: self.as_object(),
                action: action
            ];
            let _: () = msg_send![button, setKeyEquivalent: ns_string(key)];
            button
        }
    }

    // Returns an autoreleased grid of a label, slider and value label for each field.
    unsafe fn slider_grid(&self, fields: &[Field]) -> *mut Object {
        let state = self.ivars().state.get();
        // SAFETY: Every control is an autoreleased AppKit object that the grid retains.
        unsafe {
            let rows: Vec<*mut Object> = fields
                .iter()
                .map(|&field| {
                    let (min, max) = field.range();
                    let slider: *mut Object = msg_send![class!(NSSlider),
                        sliderWithValue: state.get(field),
                        minValue: min,
                        maxValue: max,
                        target: self.as_object(),
                        action: sel!(sliderChanged:)
                    ];
                    let _: () = msg_send![slider, setTag: field.tag()];
                    let _: () = msg_send![slider, setContinuous: Bool::YES];
                    let width: *mut Object = msg_send![slider, widthAnchor];
                    let width: *mut Object =
                        msg_send![width, constraintEqualToConstant: SLIDER_WIDTH];
                    let _: () = msg_send![width, setActive: Bool::YES];

                    let value = Self::label(&field.format(state.get(field)));
                    let font: *mut Object = msg_send![class!(NSFont),
                        monospacedDigitSystemFontOfSize: 0.0f64,
                        weight: 0.0f64
                    ];
                    let _: () = msg_send![value, setFont: font];
                    let _: () = msg_send![value, setAlignment: NS_TEXT_ALIGNMENT_RIGHT];
                    self.ivars().sliders.borrow_mut().push(SliderRow {
                        field,
                        slider: Retained::retain(slider).expect("slider is not null"),
                        value: Retained::retain(value).expect("label is not null"),
                    });
                    let row = [Self::label(field.label()), slider, value];
                    ns_array(&row)
                })
                .collect();
            let rows: *mut Object = ns_array(&rows);
            let grid: *mut Object = msg_send![class!(NSGridView), gridViewWithViews: rows];
            let _: () = msg_send![grid, setRowSpacing: 6.0f64];
            let _: () = msg_send![grid, setColumnSpacing: COLUMN_SPACING];
            // Fitting sizes ignore fixed column widths, so give the grid its full width.
            let width: *mut Object = msg_send![grid, widthAnchor];
            let width: *mut Object = msg_send![width, constraintEqualToConstant: GRID_WIDTH];
            let _: () = msg_send![width, setActive: Bool::YES];
            for (index, width) in [(0usize, LABEL_WIDTH), (2, VALUE_WIDTH)] {
                let column: *mut Object = msg_send![grid, columnAtIndex: index as isize];
                let _: () = msg_send![column, setWidth: width];
                let _: () = msg_send![column, setXPlacement: GRID_CELL_PLACEMENT_TRAILING];
            }
            grid
        }
    }

    // Returns an autoreleased section heading.
    unsafe fn heading(text: &str) -> *mut Object {
        // SAFETY: The label and fonts are autoreleased AppKit objects.
        unsafe {
            let label = Self::label(text);
            let font: *mut Object = msg_send![class!(NSFont), boldSystemFontOfSize: 0.0f64];
            let _: () = msg_send![label, setFont: font];
            label
        }
    }

    // Returns an autoreleased stack view of the popover contents.
    unsafe fn content(&self) -> *mut Object {
        let kind = self.ivars().kind;
        // SAFETY: Every view is an autoreleased AppKit object that its stack view retains.
        unsafe {
            let mut views = Vec::new();
            let mut last_grid = null_mut();
            if kind == PanelKind::Adjust {
                let popup: Allocated<Object> = msg_send![class!(NSPopUpButton), alloc];
                let popup: Retained<Object> = msg_send![popup,
                    initWithFrame: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size { width: 0.0, height: 0.0 },
                    },
                    pullsDown: Bool::NO
                ];
                let _: () = msg_send![&*popup, addItemWithTitle: ns_string!("None")];
                for preset in Preset::ALL {
                    let _: () = msg_send![&*popup, addItemWithTitle: ns_string(preset.name())];
                }
                let _: () = msg_send![&*popup, setTarget: self.as_object()];
                let _: () = msg_send![&*popup, setAction: sel!(presetChanged:)];
                let filter = [Self::label("Filter"), popup.as_ptr()];
                let filter: *mut Object = ns_array(&filter);
                let filter: *mut Object =
                    msg_send![class!(NSStackView), stackViewWithViews: filter];
                views.push(filter);
                self.ivars().presets.replace(Some(popup));
            }
            for (heading, fields) in kind.sections() {
                if !heading.is_empty() {
                    views.push(Self::heading(heading));
                }
                let grid = self.slider_grid(fields);
                views.push(grid);
                last_grid = grid;
            }
            if kind == PanelKind::Adjust {
                let checkboxes: Vec<*mut Object> =
                    [("Grayscale", GRAYSCALE_TAG), ("Invert Colors", INVERT_TAG)]
                        .into_iter()
                        .map(|(title, tag)| {
                            let checkbox: *mut Object = msg_send![class!(NSButton),
                                checkboxWithTitle: ns_string(title),
                                target: self.as_object(),
                                action: sel!(checkboxChanged:)
                            ];
                            let _: () = msg_send![checkbox, setTag: tag];
                            self.ivars()
                                .checkboxes
                                .borrow_mut()
                                .push(Retained::retain(checkbox).expect("checkbox is not null"));
                            checkbox
                        })
                        .collect();
                let checkboxes: *mut Object = ns_array(&checkboxes);
                views.push(msg_send![class!(NSStackView), stackViewWithViews: checkboxes]);
            }
            if self.ivars().selection.is_some() && kind != PanelKind::Straighten {
                views.push(Self::heading("Selection"));
                views.push(self.slider_grid(&[Field::Feather]));
            }

            let reset = self.button("Reset", sel!(resetValues:), "");
            let spacer: *mut Object = msg_send![class!(NSView), new];
            let spacer: *mut Object = msg_send![spacer, autorelease];
            let cancel = self.button("Cancel", sel!(cancel:), "\u{1b}");
            let apply = self.button("Apply", sel!(apply:), "\r");
            let buttons = [reset, spacer, cancel, apply];
            let buttons: *mut Object = ns_array(&buttons);
            let buttons: *mut Object = msg_send![class!(NSStackView), stackViewWithViews: buttons];
            views.push(buttons);

            let views: *mut Object = ns_array(&views);
            let stack: *mut Object = msg_send![class!(NSStackView), stackViewWithViews: views];
            let _: () =
                msg_send![stack, setOrientation: NS_USER_INTERFACE_LAYOUT_ORIENTATION_VERTICAL];
            let _: () = msg_send![stack, setAlignment: NS_LAYOUT_ATTRIBUTE_LEADING];
            let _: () = msg_send![stack, setSpacing: 12.0f64];
            let _: () = msg_send![stack, setEdgeInsets: EdgeInsets::uniform(INSET)];
            // A stack view leaves its trailing inset out of its fitting width, so set it.
            let stack_width: *mut Object = msg_send![stack, widthAnchor];
            let stack_width: *mut Object =
                msg_send![stack_width, constraintEqualToConstant: GRID_WIDTH + 2.0 * INSET];
            let _: () = msg_send![stack_width, setActive: Bool::YES];
            // The button row is as wide as the sliders, so Apply lines up with their values.
            let width: *mut Object = msg_send![buttons, widthAnchor];
            let grid_width: *mut Object = msg_send![last_grid, widthAnchor];
            let width: *mut Object = msg_send![width, constraintEqualToAnchor: grid_width];
            let _: () = msg_send![width, setActive: Bool::YES];
            stack
        }
    }
}

/// Shows a popover of `kind` for `window`, previewing on `document`, limited to `selection`.
///
/// # Safety
///
/// `document` must point to a live `Document` and `window` to its live `NSWindow`.
pub(crate) unsafe fn show_edit_panel(
    kind: PanelKind,
    document: *mut Object,
    window: *mut Object,
    selection: Option<Selection>,
) -> Option<Retained<EditPanel>> {
    // SAFETY: The caller supplies a live document and window. The popover is retained by the
    // panel, and AppKit keeps a shown popover alive until it closes.
    unsafe {
        let document_ref = &*document.cast::<Document>();
        if !document_ref.begin_preview() {
            return None;
        }
        let panel: Allocated<EditPanel> = msg_send![EditPanel::class(), alloc];
        let panel: Retained<EditPanel> = msg_send![
            super(panel.set_ivars(EditPanelIvars {
                kind,
                document: Retained::retain(document).expect("document is not null"),
                selection,
                state: Cell::new(PanelState::default()),
                sliders: RefCell::new(Vec::new()),
                checkboxes: RefCell::new(Vec::new()),
                presets: RefCell::new(None),
                popover: RefCell::new(None),
                finished: Cell::new(false),
            })),
            init
        ];

        let content = panel.content();
        // The popover takes its size from the view, which fits its contents and insets.
        let fitting: Size = msg_send![content, fittingSize];
        let _: () = msg_send![content, setFrameSize: fitting];
        let view_controller: Retained<Object> = msg_send![class!(NSViewController), new];
        let _: () = msg_send![&*view_controller, setView: content];
        let popover: Retained<Object> = msg_send![class!(NSPopover), new];
        let _: () = msg_send![&*popover, setContentViewController: view_controller.as_ptr()];
        let _: () = msg_send![&*popover, setBehavior: NS_POPOVER_BEHAVIOR_APPLICATION_DEFINED];
        let _: () = msg_send![&*popover, setDelegate: Retained::as_ptr(&panel)];
        let _: () = msg_send![&*popover, setAnimates: Bool::YES];

        let toolbar: *mut Object = msg_send![window, toolbar];
        let item = visible_item(toolbar, kind.toolbar_item());
        let anchored: Bool =
            msg_send![&*popover, respondsToSelector: sel!(showRelativeToToolbarItem:)];
        if !item.is_null() && anchored.as_bool() {
            let _: () = msg_send![&*popover, showRelativeToToolbarItem: item];
        } else {
            // Without the toolbar item, hang the popover from the top middle of the window.
            let content_view: *mut Object = msg_send![window, contentView];
            let bounds: Rect = msg_send![content_view, bounds];
            let _: () = msg_send![&*popover,
                showRelativeToRect: Rect {
                    origin: Point {
                        x: bounds.size.width / 2.0,
                        y: bounds.size.height - 1.0,
                    },
                    size: Size { width: 1.0, height: 1.0 },
                },
                ofView: content_view,
                preferredEdge: NS_RECT_EDGE_MAX_Y
            ];
        }
        panel.ivars().popover.replace(Some(popover));
        Some(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_has_its_own_tag() {
        for (index, field) in FIELDS.iter().enumerate() {
            assert_eq!(field.tag(), index as isize);
        }
    }

    #[test]
    fn values_are_shown_the_way_editors_show_them() {
        assert_eq!(Field::Exposure.format(0.0), "0");
        assert_eq!(Field::Exposure.format(0.25), "+25");
        assert_eq!(Field::Tint.format(-0.4), "-40");
        assert_eq!(Field::Blur.format(12.4), "12 px");
        assert_eq!(Field::Angle.format(-3.25), "-3.2\u{b0}");
        assert_eq!(Field::Sharpen.format(0.5), "50");
    }

    #[test]
    fn panel_state_reads_back_what_it_stores() {
        let mut state = PanelState::default();
        for (index, field) in FIELDS.into_iter().enumerate() {
            state.set(field, index as f64 / 100.0);
        }
        for (index, field) in FIELDS.into_iter().enumerate() {
            assert!((state.get(field) - index as f64 / 100.0).abs() < 1e-6);
        }
    }
}
