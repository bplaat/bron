/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;

use imageview_appkit::{CGContextFillRect, CGContextSetRGBFillColor, Point, Rect, Size};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use crate::headers::*;

// The width and height of a single checkerboard tile.
const TILE: f64 = 12.0;

define_class!(
    #[unsafe(super(NSView))]
    #[name = "ImageViewCheckerboardView"]
    struct CheckerboardView;

    impl CheckerboardView {
        #[unsafe(method(drawRect:))]
        fn _draw_rect(&self, _: Rect) {
            self.draw();
        }

        #[unsafe(method(isOpaque))]
        const fn _is_opaque(&self) -> Bool {
            Bool::YES
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn _view_did_change_effective_appearance(&self) {
            // SAFETY: self is a live NSView and the selector takes a scalar Objective-C BOOL.
            unsafe {
                let this = self as *const Self as *mut Object;
                let _: () = msg_send![this, setNeedsDisplay: Bool::YES];
            }
        }
    }
);

impl CheckerboardView {
    fn draw(&self) {
        // SAFETY: Drawing occurs during AppKit's draw pass with a live view and graphics context.
        unsafe {
            let graphics_context: *mut Object =
                msg_send![class!(NSGraphicsContext), currentContext];
            if graphics_context.is_null() {
                return;
            }
            let context: *mut c_void = msg_send![graphics_context, CGContext];
            if context.is_null() {
                return;
            }
            let this = self as *const Self as *mut Object;
            let bounds: Rect = msg_send![this, bounds];
            draw_checkerboard(context, bounds, uses_dark_appearance(this));
        }
    }
}

/// Creates an owned opaque checkerboard view that follows the effective appearance.
pub(crate) fn create_checkerboard_view(frame: Rect) -> Retained<Object> {
    // SAFETY: CheckerboardView is a registered NSView subclass initialized with a valid frame.
    unsafe {
        let view: Allocated<Object> = msg_send![CheckerboardView::class(), alloc];
        msg_send![view, initWithFrame: frame]
    }
}

const fn checker_colors(dark: bool) -> ([f64; 3], [f64; 3]) {
    if dark {
        ([0.16, 0.16, 0.17], [0.23, 0.23, 0.25])
    } else {
        ([0.94, 0.94, 0.94], [0.86, 0.86, 0.86])
    }
}

unsafe fn uses_dark_appearance(view: *mut Object) -> bool {
    // SAFETY: view is live, AppKit owns the appearance names, and the temporary array retains them
    // for the synchronous best-match query.
    unsafe {
        let appearance: *mut Object = msg_send![view, effectiveAppearance];
        let names: *mut Object = msg_send![class!(NSMutableArray), arrayWithCapacity: 2usize];
        let _: () = msg_send![names, addObject: NSAppearanceNameAqua];
        let _: () = msg_send![names, addObject: NSAppearanceNameDarkAqua];
        let best: *mut Object = msg_send![appearance, bestMatchFromAppearancesWithNames: names];
        if best.is_null() {
            return false;
        }
        let dark: Bool = msg_send![best, isEqualToString: NSAppearanceNameDarkAqua];
        dark.as_bool()
    }
}

// Returns a parity-stable centered tile grid that covers `bounds`.
fn checker_grid(bounds: Rect) -> (Point, isize, isize) {
    let center = Point {
        x: bounds.origin.x + bounds.size.width / 2.0,
        y: bounds.origin.y + bounds.size.height / 2.0,
    };
    let half_columns = (bounds.size.width / (2.0 * TILE)).ceil().max(0.0) as isize;
    let half_rows = (bounds.size.height / (2.0 * TILE)).ceil().max(0.0) as isize;
    (center, half_columns, half_rows)
}

// Returns whether the tile at the given center-relative indices gets the alternate color.
const fn tile_is_filled(row: isize, column: isize) -> bool {
    (row + column).rem_euclid(2) == 0
}

fn draw_checkerboard(context: *mut c_void, bounds: Rect, dark: bool) {
    let (base, alternate) = checker_colors(dark);
    let (center, half_columns, half_rows) = checker_grid(bounds);
    // SAFETY: context is the active CGContext and all generated rectangles are finite.
    unsafe {
        CGContextSetRGBFillColor(context, base[0], base[1], base[2], 1.0);
        CGContextFillRect(context, bounds);
        CGContextSetRGBFillColor(context, alternate[0], alternate[1], alternate[2], 1.0);
        for row in -half_rows..half_rows {
            for column in -half_columns..half_columns {
                if tile_is_filled(row, column) {
                    CGContextFillRect(
                        context,
                        Rect {
                            origin: Point {
                                x: center.x + column as f64 * TILE,
                                y: center.y + row as f64 * TILE,
                            },
                            size: Size {
                                width: TILE,
                                height: TILE,
                            },
                        },
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [f64; 6] = [1.0, 23.0, 24.0, 25.0, 48.0, 137.0];

    fn bounds_of(width: f64, height: f64) -> Rect {
        Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size { width, height },
        }
    }

    #[test]
    fn checker_grid_covers_the_bounds_from_the_center() {
        for width in SIZES {
            let bounds = bounds_of(width, 70.0);
            let (center, half_columns, half_rows) = checker_grid(bounds);
            assert!(center.x - half_columns as f64 * TILE <= bounds.origin.x);
            assert!(center.x + half_columns as f64 * TILE >= bounds.origin.x + bounds.size.width);
            assert!(center.y - half_rows as f64 * TILE <= bounds.origin.y);
            assert!(center.y + half_rows as f64 * TILE >= bounds.origin.y + bounds.size.height);
        }
    }

    #[test]
    fn tile_colors_do_not_flip_while_resizing() {
        for width in SIZES {
            let (_, half_columns, half_rows) = checker_grid(bounds_of(width, width));
            // Tile 0 is always next to the center, so center colors do not depend on the size.
            assert!((-half_columns..half_columns).contains(&0));
            assert!((-half_rows..half_rows).contains(&0));
            assert!(tile_is_filled(0, 0));
            assert!(!tile_is_filled(0, -1));
            assert!(tile_is_filled(-1, -1));
        }
    }
}
