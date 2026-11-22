/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The edit controls of the island: the edit groups in the header bar, which move to the More
//! menu when it is too narrow, the toggle buttons of the selection shapes, the overlay with the
//! crop rectangle or selection, the card of the open tool and the drags that crop or select.

use super::*;

// The edit buttons and menu items and the application actions they perform.
pub(super) const EDIT_ACTIONS: &[(&str, &str)] = &[
    ("Edit", "edit.toggle"),
    ("Done", "edit.toggle"),
    ("Undo", "edit.undo"),
    ("Redo", "edit.redo"),
    ("SelectWhole", "select.whole"),
    ("SelectRectangle", "select.rectangle"),
    ("SelectEllipse", "select.ellipse"),
    ("RotateLeft", "image.rotate_left"),
    ("RotateRight", "image.rotate_right"),
    ("FlipHorizontal", "image.flip_horizontal"),
    ("FlipVertical", "image.flip_vertical"),
    ("Crop", "image.crop"),
    ("Resize", "image.resize"),
    ("Adjust", "filter.adjust"),
    ("AutoEnhance", "filter.auto_enhance"),
    ("Grayscale", "filter.grayscale"),
    ("Sepia", "filter.sepia"),
    ("Invert", "filter.invert"),
    ("Threshold", "filter.threshold"),
    ("Solarize", "filter.solarize"),
    ("GaussianBlur", "filter.gaussian_blur"),
    ("BoxBlur", "filter.box_blur"),
    ("MotionBlur", "filter.motion_blur"),
    ("RadialBlur", "filter.radial_blur"),
    ("Sharpen", "filter.sharpen"),
    ("ReduceNoise", "filter.reduce_noise"),
    ("Pixelate", "filter.pixelate"),
    ("MoreAutoEnhance", "filter.auto_enhance"),
    ("MoreGrayscale", "filter.grayscale"),
    ("MoreSepia", "filter.sepia"),
    ("MoreInvert", "filter.invert"),
    ("MoreThreshold", "filter.threshold"),
    ("MoreSolarize", "filter.solarize"),
    ("MoreGaussianBlur", "filter.gaussian_blur"),
    ("MoreBoxBlur", "filter.box_blur"),
    ("MoreMotionBlur", "filter.motion_blur"),
    ("MoreRadialBlur", "filter.radial_blur"),
    ("MoreSharpen", "filter.sharpen"),
    ("MoreReduceNoise", "filter.reduce_noise"),
    ("MorePixelate", "filter.pixelate"),
    ("MoreRotateLeft", "image.rotate_left"),
    ("MoreRotateRight", "image.rotate_right"),
    ("MoreFlipHorizontal", "image.flip_horizontal"),
    ("MoreFlipVertical", "image.flip_vertical"),
    ("MoreSelectWhole", "select.whole"),
    ("MoreSelectRectangle", "select.rectangle"),
    ("MoreSelectEllipse", "select.ellipse"),
    ("MoreCrop", "image.crop"),
    ("MoreResize", "image.resize"),
    ("MoreAdjust", "filter.adjust"),
    // Opening the filters or More menu closes the card of the open tool, like opening a popover
    // closes the one that was open.
    ("Filters", "tool.cancel"),
    ("More", "tool.cancel"),
];

// The toggle buttons of the selection shapes.
const TOGGLES: [&str; 3] = ["SelectWhole", "SelectRectangle", "SelectEllipse"];

// The elements shown while viewing and while editing.
const VIEW_ITEMS: [&[u16]; 2] = [w!("ViewStart"), w!("ViewEnd")];
const EDIT_ITEMS: [&[u16]; 2] = [w!("EditStart"), w!("EditEnd")];

// The edit buttons that always show, and the groups that move to the More menu when the header
// bar is too narrow, from left to right, keeping selection available until last. The menu lists
// items in toolbar order; separators appear with their groups so none are left at the edges.
const EDIT_FIXED: [&[u16]; 2] = [w!("HistoryGroup"), w!("Done")];
const OVERFLOW: [(&[u16], &[&[u16]]); 4] = [
    (
        w!("TransformGroup"),
        &[
            w!("MoreRotateLeft"),
            w!("MoreRotateRight"),
            w!("MoreFlipHorizontal"),
            w!("MoreFlipVertical"),
        ],
    ),
    (
        w!("ToolsGroup"),
        &[
            w!("MoreTransformSeparator"),
            w!("MoreCrop"),
            w!("MoreResize"),
            w!("MoreAdjust"),
        ],
    ),
    (w!("Filters"), &[w!("MoreFilters")]),
    (
        w!("SelectGroup"),
        &[
            w!("MoreSelectWhole"),
            w!("MoreSelectRectangle"),
            w!("MoreSelectEllipse"),
            w!("MoreSelectSeparator"),
        ],
    ),
];

// VirtualKeyModifiers.Shift
const SHIFT: u32 = 4;
// VirtualKey.Escape
const ESCAPE: i32 = 27;

// The More button of the edit groups that do not fit.
const MORE_WIDTH: f64 = 40.0;
// The room for a few characters of the title and its margins, edit groups move to the More menu
// before the title gets narrower.
const TITLE_WIDTH: f64 = 120.0;

// A group of edit buttons that moves to the More menu when the header bar is too narrow.
struct Overflow {
    group: ComPtr,
    // Its items in the More menu.
    items: Vec<ComPtr>,
    // Its width in logical pixels when it was last shown.
    width: Cell<f64>,
    hidden: Cell<bool>,
}

// The edit groups in the header bar.
pub(super) struct EditRegions {
    // The edit buttons that always show, the groups that move to the More menu in order, and the
    // More button.
    fixed: Vec<ComPtr>,
    overflow: Vec<Overflow>,
    more: ComPtr,
    editing: Cell<bool>,
}

impl EditRegions {
    pub(super) fn find(root: &ComPtr) -> Result<Self> {
        Ok(Self {
            fixed: EDIT_FIXED
                .iter()
                .map(|name| find_name(root, name, &IID_IFRAMEWORK_ELEMENT))
                .collect::<Result<Vec<_>>>()?,
            overflow: OVERFLOW
                .iter()
                .map(|(group, items)| {
                    Ok(Overflow {
                        group: find_name(root, group, &IID_IFRAMEWORK_ELEMENT)?,
                        items: elements(root, items)?,
                        width: Cell::new(0.0),
                        hidden: Cell::new(false),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            more: find_name(root, w!("More"), &IID_IUI_ELEMENT)?,
            editing: Cell::new(false),
        })
    }

    // Fits the edit groups in the header bar of `width` logical pixels without the caption
    // buttons, while editing.
    pub(super) fn update(&self, width: f64) {
        if self.editing.get() {
            self.fit(width - BUTTONS_LEFT - END_GAP - TITLE_WIDTH);
        }
    }

    // Moves the edit groups that do not fit in `available` logical pixels to the More menu, the
    // first ones first.
    fn fit(&self, available: f64) {
        for overflow in &self.overflow {
            let width = actual_width(&overflow.group);
            if !overflow.hidden.get() && width > 0.0 {
                overflow.width.set(width);
            }
        }
        let fixed: f64 = self
            .fixed
            .iter()
            .map(|element| actual_width(element) + GROUP_SPACING)
            .sum();
        let hide = (0..=self.overflow.len())
            .find(|&count| {
                let shown: f64 = self.overflow[count..]
                    .iter()
                    .map(|overflow| overflow.width.get() + GROUP_SPACING)
                    .sum();
                let more = if count > 0 {
                    MORE_WIDTH + GROUP_SPACING
                } else {
                    0.0
                };
                fixed + shown + more <= available
            })
            .unwrap_or(self.overflow.len());
        for (index, overflow) in self.overflow.iter().enumerate() {
            let hidden = index < hide;
            if overflow.hidden.replace(hidden) != hidden {
                set_visible(&overflow.group, !hidden);
                for item in &overflow.items {
                    set_visible(item, hidden);
                }
            }
        }
        set_visible(&self.more, hide > 0);
    }
}

// Finds elements by their names.
fn elements(root: &ComPtr, names: &[&[u16]]) -> Result<Vec<ComPtr>> {
    names
        .iter()
        .map(|name| find_name(root, name, &IID_IUI_ELEMENT))
        .collect()
}

/// The edit elements of the island.
pub(super) struct EditElements {
    overlay: ComPtr,
    tool: ComPtr,
    view_items: Vec<ComPtr>,
    edit_items: Vec<ComPtr>,
    edit_button: ComPtr,
    toggles: Vec<(&'static str, ComPtr)>,
    // The action of the toggle button that is on.
    pub(super) toggled: Rc<Cell<&'static str>>,
    // Whether the card of a tool shows.
    pub(super) tool_open: Rc<Cell<bool>>,
}

impl EditElements {
    pub(super) fn find(root: &ComPtr) -> Result<Self> {
        let toggles = TOGGLES
            .iter()
            .map(|name| {
                let action = EDIT_ACTIONS
                    .iter()
                    .find(|(button, _)| button == name)
                    .map_or("", |(_, action)| *action);
                find_name(root, &wide(name), &IID_ITOGGLE_BUTTON).map(|toggle| (action, toggle))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            overlay: find_name(root, w!("Overlay"), &IID_IIMAGE)?,
            tool: find_name(root, w!("Tool"), &IID_IBORDER)?,
            view_items: elements(root, &VIEW_ITEMS)?,
            edit_items: elements(root, &EDIT_ITEMS)?,
            edit_button: find_name(root, w!("Edit"), &IID_ICONTROL)?,
            toggles,
            toggled: Rc::new(Cell::new("")),
            tool_open: Rc::new(Cell::new(false)),
        })
    }
}

/// Follows a click of the button or menu item `name` of `action`, returns whether the action
/// runs.
pub(super) fn clicked(
    sender: &ComPtr,
    name: &str,
    action: &str,
    toggled: &Cell<&'static str>,
    opened: &Cell<bool>,
) -> bool {
    // A toggle button shows what the application selects rather than the click: it stays on
    // when clicked while on, and stays as it was while the application ignores the action, like
    // while an edit is applied.
    if TOGGLES.contains(&name)
        && let Ok(toggle) = sender.cast(&IID_ITOGGLE_BUTTON)
        && let Some(checked) = boxed_bool(action == toggled.get())
    {
        // SAFETY: The sender was queried for IToggleButton and checked owns the boxed boolean.
        _ = unsafe { check(call!(toggle, IToggleButton::SetIsChecked, checked.as_raw())) };
    }
    // Canceling without an open tool would select the whole image again.
    action != "tool.cancel" || opened.get()
}

/// Escape in the image edits the whole image again, like on the other platforms. The card of an
/// open tool cancels it with its own keyboard accelerator instead.
pub(super) fn add_escape(
    scroller: &ComPtr,
    opened: Rc<Cell<bool>>,
    on_action: Rc<dyn Fn(&'static str)>,
) -> Result<()> {
    let key_down = event_handler(IID_KEY_EVENT_HANDLER, move |_, args| {
        let Some(args) = args.and_then(|args| args.cast(&IID_IKEY_ROUTED_EVENT_ARGS).ok()) else {
            return;
        };
        let mut key = 0i32;
        // SAFETY: The event arguments were queried for IKeyRoutedEventArgs and key is writable.
        unsafe {
            call!(args, IKeyRoutedEventArgs::GetKey, &mut key);
            if key == ESCAPE && !opened.get() {
                call!(args, IKeyRoutedEventArgs::SetHandled, 1u8);
                on_action("tool.cancel");
            }
        }
    });
    scroller
        .cast(&IID_IUI_ELEMENT)
        .and_then(|element| add_event!(&element, IUIElement::AddKeyDown, &key_down))
}

/// A drag with the primary mouse button, which crops or selects, at a position in the visible
/// area.
pub(crate) enum Mark {
    Begin((f64, f64)),
    // Whether shift is held, which keeps a square.
    Drag((f64, f64), bool),
    End,
}

// The drags that crop or select, which take a drag before it pans.
#[derive(Clone)]
pub(super) struct Marks {
    on_mark: Rc<dyn Fn(Mark) -> bool>,
    // Whether a drag crops or selects.
    marking: Rc<Cell<bool>>,
}

impl Marks {
    pub(super) fn new(on_mark: impl Fn(Mark) -> bool + 'static) -> Self {
        Self {
            on_mark: Rc::new(on_mark),
            marking: Rc::new(Cell::new(false)),
        }
    }

    // Starts a drag at a point, returns whether it crops or selects.
    pub(super) fn begin(&self, point: Point) -> bool {
        let marking = (self.on_mark)(Mark::Begin(position(point)));
        self.marking.set(marking);
        marking
    }

    // Follows a drag to a point, returns whether it crops or selects.
    pub(super) fn drag(&self, point: Point, args: &ComPtr) -> bool {
        if !self.marking.get() {
            return false;
        }
        let mut modifiers = 0u32;
        // SAFETY: The event arguments were queried for IPointerRoutedEventArgs and modifiers is writable.
        unsafe {
            call!(
                args,
                IPointerRoutedEventArgs::GetKeyModifiers,
                &mut modifiers
            )
        };
        (self.on_mark)(Mark::Drag(position(point), modifiers & SHIFT != 0));
        true
    }

    // Ends a drag.
    pub(super) fn end(&self) {
        if self.marking.replace(false) {
            (self.on_mark)(Mark::End);
        }
    }
}

// A pointer position in logical pixels.
fn position(point: Point) -> (f64, f64) {
    (f64::from(point.x), f64::from(point.y))
}

impl Island {
    /// Shows the edit controls in the header bar, or the view controls.
    pub(crate) fn set_editing(&self, editing: bool) {
        for (items, visible) in [
            (&self.edit.view_items, !editing),
            (&self.edit.edit_items, editing),
        ] {
            for item in items {
                set_visible(item, visible);
            }
        }
        self.regions.edit.editing.set(editing);
        self.regions.update();
    }

    /// Enables the Edit button, only still bitmaps can be edited.
    pub(crate) fn set_editable(&self, editable: bool) {
        // SAFETY: The edit button retains its IControl interface on this XAML thread.
        unsafe { call!(self.edit.edit_button, IControl::SetIsEnabled, editable) };
    }

    /// Shows the toggle button of a selection action as on and the others as off.
    pub(crate) fn set_toggled(&self, action: &str) {
        for (toggle_action, toggle) in &self.edit.toggles {
            if *toggle_action == action {
                self.edit.toggled.set(toggle_action);
            }
            if let Some(checked) = boxed_bool(*toggle_action == action) {
                // SAFETY: The queried toggle and its boxed boolean live through the call.
                _ = unsafe { check(call!(toggle, IToggleButton::SetIsChecked, checked.as_raw())) };
            }
        }
    }

    /// Shows a bitmap over the visible area at its size in logical pixels, or nothing.
    pub(crate) fn set_overlay(&self, overlay: Option<(&Bitmap, (f64, f64))>) {
        if let Some((bitmap, size)) = overlay {
            set_image(&self.edit.overlay, bitmap, size);
        } else {
            // Hidden image elements otherwise retain their last CPU buffer and GPU texture.
            // SAFETY: The overlay retains IImage and accepts null to clear its source.
            unsafe { check(call!(self.edit.overlay, IImage::SetSource, null_mut())) }.ok();
        }
        set_visible(&self.edit.overlay, overlay.is_some());
    }

    /// Shows the controls of an edit tool, or hides them.
    pub(crate) fn set_tool(&self, content: Option<&ComPtr>) {
        let element = content.and_then(|content| content.cast(&IID_IUI_ELEMENT).ok());
        // SAFETY: The border accepts the queried IUIElement or null to clear its child.
        _ = unsafe {
            check(call!(
                self.edit.tool,
                IBorder::SetChild,
                element.as_ref().map_or(null_mut(), ComPtr::as_raw)
            ))
        };
        set_visible(&self.edit.tool, element.is_some());
        self.edit.tool_open.set(element.is_some());
    }

    #[cfg(test)]
    pub(crate) fn assert_header_layout(&self, filters_overflowed: Option<bool>) {
        let (width, _, scale) = self.regions.size.get();
        let mut inset = 0;
        // SAFETY: The title bar retains its IAppWindowTitleBar and inset is writable.
        unsafe {
            call!(
                self.title_bar,
                IAppWindowTitleBar::GetRightInset,
                &mut inset
            )
        };
        let controls = BUTTONS_LEFT
            + actual_width(&self.regions.start)
            + actual_width(&self.regions.end)
            + END_GAP;
        assert!(
            controls <= f64::from(width - inset) / scale + 1.0,
            "header controls overlap at width {width}, scale {scale}: {controls}"
        );
        if let Some(hidden) = filters_overflowed {
            for (name, expected) in [(w!("Filters"), hidden), (w!("MoreFilters"), !hidden)] {
                let element = find_name(&self.root, name, &IID_IUI_ELEMENT).unwrap();
                let mut visibility = -1;
                // SAFETY: The menu element was queried for IUIElement and visibility is writable.
                unsafe { call!(element, IUIElement::GetVisibility, &mut visibility) };
                assert_eq!(visibility, i32::from(expected), "filter menu visibility");
            }
            let selection = find_name(&self.root, w!("SelectGroup"), &IID_IUI_ELEMENT).unwrap();
            let mut visibility = -1;
            // SAFETY: The selection element was queried for IUIElement and visibility is writable.
            unsafe { call!(selection, IUIElement::GetVisibility, &mut visibility) };
            if visibility == 1 {
                assert!(
                    self.regions
                        .edit
                        .overflow
                        .iter()
                        .all(|group| group.hidden.get()),
                    "selection must collapse only after every other group"
                );
            }
        }
    }
}
