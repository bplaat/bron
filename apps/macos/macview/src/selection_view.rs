/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The overlay that draws and edits the selection on top of the media.

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;

use macview_appkit::{NS_VIEW_HEIGHT_SIZABLE, NS_VIEW_WIDTH_SIZABLE, Point, Rect, Size};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use crate::edit::{Selection, SelectionShape};
use crate::headers::*;

// The size of a resize handle and the distance it can be grabbed from, in window points.
const HANDLE_SIZE: f64 = 7.0;
const HANDLE_REACH: f64 = 6.0;
// Selections smaller than this in either direction, in pixels, are dropped on mouse up.
const MINIMUM_SELECTION: f64 = 2.0;

/// What dragging over the media does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    /// Drags the media around.
    Pan,
    /// Draws a rectangular selection.
    Rectangle,
    /// Draws an elliptical selection.
    Ellipse,
}

impl Tool {
    // Returns the shape the tool selects, or `None` for the pan tool.
    const fn shape(self) -> Option<SelectionShape> {
        match self {
            Self::Pan => None,
            Self::Rectangle => Some(SelectionShape::Rectangle),
            Self::Ellipse => Some(SelectionShape::Ellipse),
        }
    }
}

// The edges of the selection that a handle moves.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Edges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

// The handles around a selection, as the edges each one moves.
const HANDLES: [Edges; 8] = [
    Edges::new(true, false, true, false),
    Edges::new(false, false, true, false),
    Edges::new(false, true, true, false),
    Edges::new(false, true, false, false),
    Edges::new(false, true, false, true),
    Edges::new(false, false, false, true),
    Edges::new(true, false, false, true),
    Edges::new(true, false, false, false),
];

impl Edges {
    const fn new(left: bool, right: bool, top: bool, bottom: bool) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    // Returns where the handle of these edges sits on `selection`.
    fn position(self, selection: &Selection) -> Point {
        let x = if self.left {
            selection.x
        } else if self.right {
            selection.x + selection.width
        } else {
            selection.x + selection.width / 2.0
        };
        let y = if self.top {
            selection.y
        } else if self.bottom {
            selection.y + selection.height
        } else {
            selection.y + selection.height / 2.0
        };
        Point { x, y }
    }

    const fn is_corner(self) -> bool {
        (self.left || self.right) && (self.top || self.bottom)
    }
}

// The kind of change a mouse drag makes, with the state it started from.
#[derive(Clone, Copy)]
enum Drag {
    Create { anchor: Point },
    Move { start: Point, original: Selection },
    Resize { edges: Edges, original: Selection },
}

pub(crate) struct SelectionViewIvars {
    tool: Cell<Tool>,
    selection: Cell<Option<Selection>>,
    // The size of the edited bitmap, which the selection is measured in.
    canvas_size: Cell<Size>,
    drag: Cell<Option<Drag>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[name = "MacViewSelectionView"]
    #[ivars = SelectionViewIvars]
    pub(crate) struct SelectionView;

    impl SelectionView {
        // Flipped so the view measures from the top, like the pixel rows of the bitmap.
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }

        // Clicks only reach the overlay through the clip view while a selection tool is active.
        #[unsafe(method(hitTest:))]
        const fn _hit_test(&self, _: Point) -> *mut Object {
            null_mut()
        }

        #[unsafe(method(drawRect:))]
        fn _draw_rect(&self, _: Rect) {
            self.draw();
        }

        #[unsafe(method(updateTrackingAreas))]
        fn _update_tracking_areas(&self) {
            self.update_tracking_areas();
        }

        #[unsafe(method(cursorUpdate:))]
        fn _cursor_update(&self, event: *mut Object) {
            self.update_cursor(event);
        }

        #[unsafe(method(mouseMoved:))]
        fn _mouse_moved(&self, event: *mut Object) {
            self.update_cursor(event);
        }

        #[unsafe(method(mouseDown:))]
        fn _mouse_down(&self, event: *mut Object) {
            self.mouse_down(event);
        }

        #[unsafe(method(mouseDragged:))]
        fn _mouse_dragged(&self, event: *mut Object) {
            self.mouse_dragged(event);
        }

        #[unsafe(method(mouseUp:))]
        fn _mouse_up(&self, _: *mut Object) {
            self.mouse_up();
        }
    }
);

impl SelectionView {
    /// Returns the active tool.
    pub(crate) fn tool(&self) -> Tool {
        self.ivars().tool.get()
    }

    /// Switches tools, keeping the selection.
    pub(crate) fn set_tool(&self, tool: Tool) {
        self.ivars().tool.set(tool);
        self.redraw();
        // SAFETY: The window, if any, keeps its views alive while it resets their cursors.
        unsafe {
            let window: *mut Object = msg_send![self.as_object(), window];
            let clip_view = self.clip_view();
            if !window.is_null() && !clip_view.is_null() {
                let _: () = msg_send![window, invalidateCursorRectsForView: clip_view];
            }
        }
    }

    /// Returns the selection in pixels of the bitmap.
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.ivars().selection.get()
    }

    /// Replaces the selection, or clears it with `None`.
    pub(crate) fn set_selection(&self, selection: Option<Selection>) {
        self.ivars().selection.set(selection);
        self.redraw();
    }

    /// Sets the size of the bitmap, which clears the selection when it changes.
    pub(crate) fn set_canvas_size(&self, size: Size) {
        let previous = self.ivars().canvas_size.replace(size);
        if previous.width != size.width || previous.height != size.height {
            self.set_selection(None);
        }
    }

    /// Returns whether the overlay takes the mouse instead of the clip view.
    pub(crate) fn is_selecting(&self) -> bool {
        self.tool() != Tool::Pan
    }

    const fn as_object(&self) -> *mut Object {
        self as *const Self as *mut Object
    }

    fn redraw(&self) {
        // SAFETY: The view is live and the selector takes a scalar Objective-C BOOL.
        unsafe {
            let _: () = msg_send![self.as_object(), setNeedsDisplay: Bool::YES];
        }
    }

    // Returns the clip view the overlay is shown in, or null outside of a scroll view.
    fn clip_view(&self) -> *mut Object {
        // SAFETY: The superview chain is live while the view is.
        unsafe {
            let scroll_view: *mut Object = msg_send![self.as_object(), enclosingScrollView];
            if scroll_view.is_null() {
                return null_mut();
            }
            msg_send![scroll_view, contentView]
        }
    }

    // Returns how many window points one point of the overlay takes up.
    fn magnification(&self) -> f64 {
        // SAFETY: The scroll view around the overlay is live while the overlay is.
        unsafe {
            let scroll_view: *mut Object = msg_send![self.as_object(), enclosingScrollView];
            if scroll_view.is_null() {
                return 1.0;
            }
            let magnification: f64 = msg_send![scroll_view, magnification];
            if magnification > 0.0 {
                magnification
            } else {
                1.0
            }
        }
    }

    // Returns the pixels per overlay point in both directions.
    fn pixel_scale(&self) -> Size {
        // SAFETY: The view is live.
        let bounds: Rect = unsafe { msg_send![self.as_object(), bounds] };
        let canvas = self.ivars().canvas_size.get();
        Size {
            width: canvas.width / bounds.size.width.max(1.0),
            height: canvas.height / bounds.size.height.max(1.0),
        }
    }

    // Returns where an event happened, in pixels of the bitmap.
    fn event_pixel(&self, event: *mut Object) -> Point {
        // SAFETY: AppKit passes a valid NSEvent of the window the overlay is in.
        let point: Point = unsafe {
            let location: Point = msg_send![event, locationInWindow];
            msg_send![self.as_object(), convertPoint: location, fromView: null_mut::<Object>()]
        };
        let scale = self.pixel_scale();
        Point {
            x: point.x * scale.width,
            y: point.y * scale.height,
        }
    }

    // Returns the overlay rectangle that covers `selection`.
    fn view_rect(&self, selection: &Selection) -> Rect {
        let scale = self.pixel_scale();
        Rect {
            origin: Point {
                x: selection.x / scale.width,
                y: selection.y / scale.height,
            },
            size: Size {
                width: selection.width / scale.width,
                height: selection.height / scale.height,
            },
        }
    }

    // Returns the handle under `pixel`, if the selection has one there.
    fn handle_at(&self, selection: &Selection, pixel: Point) -> Option<Edges> {
        let scale = self.pixel_scale();
        let reach = HANDLE_REACH / self.magnification();
        HANDLES.into_iter().find(|edges| {
            let handle = edges.position(selection);
            ((handle.x - pixel.x) / scale.width).abs() <= reach
                && ((handle.y - pixel.y) / scale.height).abs() <= reach
        })
    }

    fn contains(selection: &Selection, pixel: Point) -> bool {
        pixel.x >= selection.x
            && pixel.x <= selection.x + selection.width
            && pixel.y >= selection.y
            && pixel.y <= selection.y + selection.height
    }

    // Returns `point` moved inside the bitmap.
    fn clamp_to_canvas(&self, point: Point) -> Point {
        let canvas = self.ivars().canvas_size.get();
        Point {
            x: point.x.clamp(0.0, canvas.width),
            y: point.y.clamp(0.0, canvas.height),
        }
    }

    fn draw(&self) {
        let Some(selection) = self.selection() else {
            return;
        };
        let rect = self.view_rect(&selection);
        let line = 1.0 / self.magnification();
        // SAFETY: Drawing happens during the draw pass of the view. The colors and paths are
        // autoreleased objects of AppKit, and the dash pattern outlives the call that copies it.
        unsafe {
            let bounds: Rect = msg_send![self.as_object(), bounds];
            let shape: *mut Object = match selection.shape {
                SelectionShape::Rectangle => {
                    msg_send![class!(NSBezierPath), bezierPathWithRect: rect]
                }
                SelectionShape::Ellipse => {
                    msg_send![class!(NSBezierPath), bezierPathWithOvalInRect: rect]
                }
            };

            // Dim everything outside the selection, which edits leave alone.
            let outside: *mut Object = msg_send![class!(NSBezierPath), bezierPathWithRect: bounds];
            let _: () = msg_send![outside, appendBezierPath: shape];
            let _: () = msg_send![outside, setWindingRule: NS_WINDING_RULE_EVEN_ODD];
            let dim: *mut Object =
                msg_send![class!(NSColor), colorWithWhite: 0.0f64, alpha: 0.35f64];
            let _: () = msg_send![dim, setFill];
            let _: () = msg_send![outside, fill];

            // Marching ants: a white line with a black dash on top, readable on any image.
            let _: () = msg_send![shape, setLineWidth: line];
            let white: *mut Object = msg_send![class!(NSColor), whiteColor];
            let _: () = msg_send![white, setStroke];
            let _: () = msg_send![shape, stroke];
            let dash = [4.0 * line, 4.0 * line];
            let _: () = msg_send![shape,
                setLineDash: dash.as_ptr().cast::<c_void>(),
                count: dash.len() as isize,
                phase: 0.0f64
            ];
            let black: *mut Object = msg_send![class!(NSColor), blackColor];
            let _: () = msg_send![black, setStroke];
            let _: () = msg_send![shape, stroke];

            if !self.is_selecting() {
                return;
            }
            let size = HANDLE_SIZE * line;
            let gray: *mut Object =
                msg_send![class!(NSColor), colorWithWhite: 0.3f64, alpha: 1.0f64];
            for edges in HANDLES {
                let center = edges.position(&self.view_selection(&selection, rect));
                let handle: *mut Object = msg_send![class!(NSBezierPath),
                    bezierPathWithRect: Rect {
                        origin: Point {
                            x: center.x - size / 2.0,
                            y: center.y - size / 2.0,
                        },
                        size: Size {
                            width: size,
                            height: size,
                        },
                    }
                ];
                let _: () = msg_send![handle, setLineWidth: line];
                let _: () = msg_send![white, setFill];
                let _: () = msg_send![handle, fill];
                let _: () = msg_send![gray, setStroke];
                let _: () = msg_send![handle, stroke];
            }
        }
    }

    // Returns the selection measured in overlay points, to place handles with.
    const fn view_selection(&self, selection: &Selection, rect: Rect) -> Selection {
        Selection {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
            ..*selection
        }
    }

    fn update_tracking_areas(&self) {
        // SAFETY: The view owns its tracking areas, and the new one is released after the view
        // retains it.
        unsafe {
            let this = self.as_object();
            let areas: *mut Object = msg_send![this, trackingAreas];
            let areas: Retained<Object> = msg_send![areas, copy];
            let count: usize = msg_send![&*areas, count];
            for index in 0..count {
                let area: *mut Object = msg_send![&*areas, objectAtIndex: index];
                let _: () = msg_send![this, removeTrackingArea: area];
            }
            let area: Allocated<Object> = msg_send![class!(NSTrackingArea), alloc];
            let area: Retained<Object> = msg_send![area,
                initWithRect: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                },
                options: NS_TRACKING_MOUSE_MOVED
                    | NS_TRACKING_CURSOR_UPDATE
                    | NS_TRACKING_ACTIVE_IN_KEY_WINDOW
                    | NS_TRACKING_IN_VISIBLE_RECT,
                owner: this,
                userInfo: null_mut::<Object>()
            ];
            let _: () = msg_send![this, addTrackingArea: area.as_ptr()];
            let _: () = msg_send![super(self), updateTrackingAreas];
        }
    }

    // Shows what a click would do: move the selection, resize it, or draw a new one.
    fn update_cursor(&self, event: *mut Object) {
        if !self.is_selecting() {
            return;
        }
        let pixel = self.event_pixel(event);
        // SAFETY: The cursors are shared objects of AppKit.
        unsafe {
            let cursor: *mut Object = match self.selection() {
                Some(selection) => match self.handle_at(&selection, pixel) {
                    Some(edges) if edges.is_corner() => {
                        msg_send![class!(NSCursor), crosshairCursor]
                    }
                    Some(edges) if edges.left || edges.right => {
                        msg_send![class!(NSCursor), resizeLeftRightCursor]
                    }
                    Some(_) => msg_send![class!(NSCursor), resizeUpDownCursor],
                    None if Self::contains(&selection, pixel) => {
                        msg_send![class!(NSCursor), openHandCursor]
                    }
                    None => msg_send![class!(NSCursor), crosshairCursor],
                },
                None => msg_send![class!(NSCursor), crosshairCursor],
            };
            let _: () = msg_send![cursor, set];
        }
    }

    fn mouse_down(&self, event: *mut Object) {
        let Some(shape) = self.tool().shape() else {
            return;
        };
        let pixel = self.event_pixel(event);
        let drag = match self.selection() {
            Some(selection) => match self.handle_at(&selection, pixel) {
                Some(edges) => Drag::Resize {
                    edges,
                    original: selection,
                },
                None if Self::contains(&selection, pixel) => {
                    // SAFETY: The closed hand cursor is a shared object of AppKit.
                    unsafe {
                        let cursor: *mut Object = msg_send![class!(NSCursor), closedHandCursor];
                        let _: () = msg_send![cursor, set];
                    }
                    Drag::Move {
                        start: pixel,
                        original: selection,
                    }
                }
                None => Drag::Create {
                    anchor: self.clamp_to_canvas(pixel),
                },
            },
            None => Drag::Create {
                anchor: self.clamp_to_canvas(pixel),
            },
        };
        if let Drag::Create { anchor } = drag {
            self.set_selection(Some(Selection {
                shape,
                x: anchor.x,
                y: anchor.y,
                width: 0.0,
                height: 0.0,
                feather: 0.0,
            }));
        }
        self.ivars().drag.set(Some(drag));
    }

    fn mouse_dragged(&self, event: *mut Object) {
        let (Some(drag), Some(selection)) = (self.ivars().drag.get(), self.selection()) else {
            return;
        };
        let pixel = self.event_pixel(event);
        // SAFETY: AppKit passes a valid NSEvent.
        let modifiers: u64 = unsafe { msg_send![event, modifierFlags] };
        let square = modifiers & NS_EVENT_MODIFIER_FLAG_SHIFT != 0;
        let centered = modifiers & NS_EVENT_MODIFIER_FLAG_OPTION != 0;
        let canvas = self.ivars().canvas_size.get();
        let updated = match drag {
            Drag::Create { anchor } => {
                let mut dx = pixel.x - anchor.x;
                let mut dy = pixel.y - anchor.y;
                if square {
                    let side = dx.abs().max(dy.abs());
                    dx = side.copysign(dx);
                    dy = side.copysign(dy);
                }
                let (left, top, right, bottom) = if centered {
                    (
                        anchor.x - dx.abs(),
                        anchor.y - dy.abs(),
                        anchor.x + dx.abs(),
                        anchor.y + dy.abs(),
                    )
                } else {
                    (
                        anchor.x.min(anchor.x + dx),
                        anchor.y.min(anchor.y + dy),
                        anchor.x.max(anchor.x + dx),
                        anchor.y.max(anchor.y + dy),
                    )
                };
                clamp_rect(selection, left, top, right, bottom, canvas)
            }
            Drag::Move { start, original } => Selection {
                x: (original.x + pixel.x - start.x)
                    .clamp(0.0, (canvas.width - original.width).max(0.0)),
                y: (original.y + pixel.y - start.y)
                    .clamp(0.0, (canvas.height - original.height).max(0.0)),
                ..original
            },
            Drag::Resize { edges, original } => {
                resize(original, edges, self.clamp_to_canvas(pixel), square, canvas)
            }
        };
        self.set_selection(Some(updated));
    }

    fn mouse_up(&self) {
        if self.ivars().drag.take().is_none() {
            return;
        }
        // A click without a drag clears the selection.
        if let Some(selection) = self.selection()
            && (selection.width < MINIMUM_SELECTION || selection.height < MINIMUM_SELECTION)
        {
            self.set_selection(None);
        }
    }
}

// Returns `selection` with the given edges, cut off at the edges of the bitmap.
fn clamp_rect(
    selection: Selection,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    canvas: Size,
) -> Selection {
    let left = left.clamp(0.0, canvas.width);
    let top = top.clamp(0.0, canvas.height);
    Selection {
        x: left,
        y: top,
        width: right.clamp(0.0, canvas.width) - left,
        height: bottom.clamp(0.0, canvas.height) - top,
        ..selection
    }
}

// Moves the dragged edges of `original` to `pixel`, keeping its shape with `square` on a corner.
fn resize(
    original: Selection,
    edges: Edges,
    pixel: Point,
    square: bool,
    canvas: Size,
) -> Selection {
    let mut left = original.x;
    let mut top = original.y;
    let mut right = original.x + original.width;
    let mut bottom = original.y + original.height;
    if edges.left {
        left = pixel.x;
    }
    if edges.right {
        right = pixel.x;
    }
    if edges.top {
        top = pixel.y;
    }
    if edges.bottom {
        bottom = pixel.y;
    }
    if square && edges.is_corner() && original.height > 0.0 {
        // Grow the shorter side to keep the aspect ratio the selection started with.
        let ratio = original.width / original.height;
        let width = (right - left).abs();
        let height = (bottom - top).abs();
        if width / ratio > height {
            let height = width / ratio;
            if edges.top {
                top = bottom - height.copysign(bottom - top);
            } else {
                bottom = top + height.copysign(bottom - top);
            }
        } else {
            let width = height * ratio;
            if edges.left {
                left = right - width.copysign(right - left);
            } else {
                right = left + width.copysign(right - left);
            }
        }
    }
    clamp_rect(
        original,
        left.min(right),
        top.min(bottom),
        left.max(right),
        top.max(bottom),
        canvas,
    )
}

/// Creates an owned overlay with no selection and the pan tool.
pub(crate) fn create_selection_view() -> Retained<SelectionView> {
    // SAFETY: SelectionView is a registered NSView subclass whose ivars are set before init.
    unsafe {
        let view: Allocated<SelectionView> = msg_send![SelectionView::class(), alloc];
        let view: Retained<SelectionView> = msg_send![
            super(view.set_ivars(SelectionViewIvars {
                tool: Cell::new(Tool::Pan),
                selection: Cell::new(None),
                canvas_size: Cell::new(Size {
                    width: 0.0,
                    height: 0.0,
                }),
                drag: Cell::new(None),
            })),
            initWithFrame: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 0.0,
                    height: 0.0,
                },
            }
        ];
        let _: () = msg_send![&*view,
            setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
        ];
        view
    }
}

/// Moves the overlay onto `media_view`, covering it, for a bitmap of `canvas_size` pixels.
///
/// # Safety
///
/// `media_view` must point to a live `NSView`.
pub(crate) unsafe fn attach_selection_view(
    overlay: &SelectionView,
    media_view: *mut Object,
    canvas_size: Size,
) {
    // SAFETY: The caller supplies a live view, which retains the overlay as its subview.
    unsafe {
        let bounds: Rect = msg_send![media_view, bounds];
        let overlay_object = overlay as *const SelectionView as *mut Object;
        let _: () = msg_send![overlay_object, setFrame: bounds];
        let _: () = msg_send![media_view, addSubview: overlay_object];
    }
    overlay.set_canvas_size(canvas_size);
}

/// Returns the overlay on top of `media_view`, if it has one.
///
/// # Safety
///
/// `media_view` must be null or point to a live `NSView`.
pub(crate) unsafe fn selection_view_of(media_view: *mut Object) -> Option<Retained<SelectionView>> {
    if media_view.is_null() {
        return None;
    }
    // SAFETY: The caller supplies a live view, and the class check makes the cast sound.
    unsafe {
        let subviews: *mut Object = msg_send![media_view, subviews];
        let count: usize = msg_send![subviews, count];
        for index in 0..count {
            let view: *mut Object = msg_send![subviews, objectAtIndex: index];
            let ours: Bool = msg_send![view, isKindOfClass: SelectionView::class()];
            if ours.as_bool() {
                return Retained::retain(view.cast::<SelectionView>());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: Size = Size {
        width: 100.0,
        height: 80.0,
    };

    fn selection(x: f64, y: f64, width: f64, height: f64) -> Selection {
        Selection {
            shape: SelectionShape::Rectangle,
            x,
            y,
            width,
            height,
            feather: 0.0,
        }
    }

    #[test]
    fn dragging_a_corner_past_the_opposite_one_flips_the_selection() {
        let resized = resize(
            selection(20.0, 20.0, 30.0, 30.0),
            HANDLES[4],
            Point { x: 10.0, y: 5.0 },
            false,
            CANVAS,
        );
        assert_eq!(resized, selection(10.0, 5.0, 10.0, 15.0));
    }

    #[test]
    fn shift_resizing_a_corner_keeps_the_aspect_ratio() {
        let resized = resize(
            selection(0.0, 0.0, 20.0, 10.0),
            HANDLES[4],
            Point { x: 40.0, y: 12.0 },
            true,
            CANVAS,
        );
        assert_eq!(resized, selection(0.0, 0.0, 40.0, 20.0));
    }

    #[test]
    fn resizing_stops_at_the_bitmap_edges() {
        let resized = resize(
            selection(50.0, 50.0, 10.0, 10.0),
            HANDLES[4],
            Point { x: 500.0, y: 500.0 },
            false,
            CANVAS,
        );
        assert_eq!(resized, selection(50.0, 50.0, 50.0, 30.0));
    }
}
