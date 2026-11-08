/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The inspector of an image window: a sidebar on the right of the split view that lists the
//! information about the image in titled sections, opened with the Info button. It starts
//! closed.

use std::cell::{Cell, RefCell};

use imageview_core::InfoSection;
use imageview_core::macos::{Rect, ns_string};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

use super::headers::*;
use super::sidebar::Sidebar;
use super::view::rect;

const NS_SPLIT_VIEW_ITEM_COLLAPSE_BEHAVIOR_PREFER_RESIZING_SPLIT_VIEW: i64 = 2;
const NS_FONT_WEIGHT_SEMIBOLD: f64 = 0.3;

// The width of the inspector and the space around and between its rows.
const MINIMUM_WIDTH: f64 = 220.0;
const WIDTH: f64 = 260.0;
const MAXIMUM_WIDTH: f64 = 420.0;
const PADDING: f64 = 16.0;
const ROW_SPACING: f64 = 10.0;
const SECTION_SPACING: f64 = 22.0;

define_class!(
    // The document of the scroll view of the inspector, flipped so it starts at the top.
    #[unsafe(super(NSView))]
    #[name = "ImageViewInspectorDocument"]
    struct Document;

    impl Document {
        #[unsafe(method(isFlipped))]
        const fn _is_flipped(&self) -> Bool {
            Bool::YES
        }
    }
);

/// The inspector split view item with the sections of the image.
pub(super) struct Inspector {
    item: Retained<Object>,
    list: Retained<Object>,
    stack: Retained<Object>,
    // Whether it is opened with the Info button.
    open: Cell<bool>,
    // Whether it is hidden while the image is edited.
    hidden: Cell<bool>,
    // The constraint that places the list below the toolbar, which goes away when the list
    // leaves the window.
    below_toolbar: RefCell<Option<Retained<Object>>>,
}

impl Inspector {
    /// Adds a closed inspector to the right of a split view controller.
    pub(super) fn new(controller: *mut Object, frame: Rect) -> Self {
        // SAFETY: All objects are valid AppKit instances, the split view controller retains the
        // item, which retains its view controller and views.
        unsafe {
            let stack: Retained<Object> = msg_send![class!(NSStackView), new];
            let _: () = msg_send![&*stack,
                setOrientation: NS_USER_INTERFACE_LAYOUT_ORIENTATION_VERTICAL
            ];
            let _: () = msg_send![&*stack, setAlignment: NS_LAYOUT_ATTRIBUTE_LEADING];
            let _: () = msg_send![&*stack, setSpacing: 2.0f64];
            let _: () = msg_send![&*stack, setTranslatesAutoresizingMaskIntoConstraints: Bool::NO];
            let document: Retained<Object> = msg_send![Document::class(), new];
            let _: () =
                msg_send![&*document, setTranslatesAutoresizingMaskIntoConstraints: Bool::NO];
            let _: () = msg_send![&*document, addSubview: &*stack];
            let list: Allocated<Object> = msg_send![class!(NSScrollView), alloc];
            let list: Retained<Object> = msg_send![list,
                initWithFrame: rect(0.0, 0.0, WIDTH, frame.size.height)
            ];
            let _: () = msg_send![&*list, setDrawsBackground: Bool::NO];
            let _: () = msg_send![&*list, setHasVerticalScroller: Bool::YES];
            let _: () = msg_send![&*list, setAutohidesScrollers: Bool::YES];
            let _: () = msg_send![&*list, setDocumentView: &*document];
            // The document is as wide as the scroll view and as high as its rows.
            let clip: *mut Object = msg_send![&*list, contentView];
            let anchors: [(*mut Object, *mut Object, f64); 7] = [
                (
                    msg_send![&*document, topAnchor],
                    msg_send![clip, topAnchor],
                    0.0,
                ),
                (
                    msg_send![&*document, leadingAnchor],
                    msg_send![clip, leadingAnchor],
                    0.0,
                ),
                (
                    msg_send![&*document, trailingAnchor],
                    msg_send![clip, trailingAnchor],
                    0.0,
                ),
                (
                    msg_send![&*stack, topAnchor],
                    msg_send![&*document, topAnchor],
                    PADDING / 2.0,
                ),
                (
                    msg_send![&*stack, leadingAnchor],
                    msg_send![&*document, leadingAnchor],
                    PADDING,
                ),
                (
                    msg_send![&*stack, trailingAnchor],
                    msg_send![&*document, trailingAnchor],
                    -PADDING,
                ),
                (
                    msg_send![&*stack, bottomAnchor],
                    msg_send![&*document, bottomAnchor],
                    -PADDING,
                ),
            ];
            for (anchor, to, constant) in anchors {
                let constraint: *mut Object =
                    msg_send![anchor, constraintEqualToAnchor: to, constant: constant];
                let _: () = msg_send![constraint, setActive: Bool::YES];
            }

            // The inspector opens as wide as its view starts.
            let container =
                super::sidebar::container(rect(0.0, 0.0, WIDTH, frame.size.height), &list);
            let view_controller = super::sidebar::view_controller(&container);
            let item: Retained<Object> = msg_send![class!(NSSplitViewItem),
                inspectorWithViewController: &*view_controller
            ];
            let _: () = msg_send![&*item, setMinimumThickness: MINIMUM_WIDTH];
            let _: () = msg_send![&*item, setMaximumThickness: MAXIMUM_WIDTH];
            super::sidebar::keep_on_window_resize(&item);
            let _: () = msg_send![&*item, setCanCollapse: Bool::YES];
            // Opening the inspector makes the window wider instead of the image smaller.
            let _: () = msg_send![&*item,
                setCollapseBehavior: NS_SPLIT_VIEW_ITEM_COLLAPSE_BEHAVIOR_PREFER_RESIZING_SPLIT_VIEW
            ];
            let _: () = msg_send![&*item, setCollapsed: Bool::YES];
            let _: () = msg_send![controller, addSplitViewItem: &*item];
            Self {
                item,
                list,
                stack,
                open: Cell::new(false),
                hidden: Cell::new(false),
                below_toolbar: RefCell::new(None),
            }
        }
    }

    /// Opens or closes the inspector.
    pub(super) fn toggle(&self) {
        // Dragging the divider can collapse the inspector, so it follows what is shown.
        // SAFETY: The item is a live split view item.
        let collapsed: Bool = unsafe { msg_send![&*self.item, isCollapsed] };
        self.open.set(collapsed.as_bool());
        self.update(true);
    }

    /// Hides the inspector while the image is edited, or shows it again when it is open.
    pub(super) fn set_hidden(&self, hidden: bool) {
        // Dragging the divider can collapse the inspector, which then stays closed.
        if hidden && !self.hidden.get() {
            // SAFETY: The item is a live split view item.
            let collapsed: Bool = unsafe { msg_send![&*self.item, isCollapsed] };
            self.open.set(!collapsed.as_bool());
        }
        self.hidden.set(hidden);
        self.update(false);
    }

    /// Whether the inspector is opened with the Info button.
    pub(super) const fn is_open(&self) -> bool {
        self.open.get()
    }

    // Collapses or expands the inspector to follow whether it is open and not hidden.
    fn update(&self, animate: bool) {
        let collapsed = objc_bool(!self.open.get() || self.hidden.get());
        // SAFETY: The item is a live split view item, its animator proxy collapses it with
        // an animation.
        unsafe {
            let current: Bool = msg_send![&*self.item, isCollapsed];
            if current == collapsed {
                return;
            }
            if animate {
                let animator: *mut Object = msg_send![&*self.item, animator];
                let _: () = msg_send![animator, setCollapsed: collapsed];
            } else {
                let _: () = msg_send![&*self.item, setCollapsed: collapsed];
            }
            if !collapsed.as_bool() {
                self.place_below_toolbar();
            }
        }
    }

    // Places the list below the toolbar once it is in the window, again after it left it.
    fn place_below_toolbar(&self) {
        let mut below_toolbar = self.below_toolbar.borrow_mut();
        let active = below_toolbar.as_ref().is_some_and(|constraint| {
            // SAFETY: The constraint is a live NSLayoutConstraint.
            let active: Bool = unsafe { msg_send![&**constraint, isActive] };
            active.as_bool()
        });
        if !active {
            *below_toolbar = Sidebar::place_below_toolbar(Retained::as_ptr(&self.list));
        }
    }

    /// Lists sections, each a title with names and values below it.
    pub(super) fn set_sections(&self, sections: &[InfoSection]) {
        // SAFETY: The stack retains its arranged views, the labels are autoreleased.
        unsafe {
            let views: *mut Object = msg_send![&*self.stack, arrangedSubviews];
            let views: Retained<Object> = msg_send![views, copy];
            let count: usize = msg_send![&*views, count];
            for index in 0..count {
                let view: *mut Object = msg_send![&*views, objectAtIndex: index];
                let _: () = msg_send![view, removeFromSuperview];
            }
            for section in sections {
                let title = self.add_label(section.title, Style::Title);
                let _: () = msg_send![&*self.stack, setCustomSpacing: 8.0f64, afterView: title];
                let mut last = title;
                for (name, value) in &section.rows {
                    self.add_label(name, Style::Name);
                    last = self.add_label(value, Style::Value);
                    let _: () =
                        msg_send![&*self.stack, setCustomSpacing: ROW_SPACING, afterView: last];
                }
                let _: () =
                    msg_send![&*self.stack, setCustomSpacing: SECTION_SPACING, afterView: last];
            }
        }
    }

    // Adds a label as wide as the stack and returns it.
    fn add_label(&self, text: &str, style: Style) -> *mut Object {
        // SAFETY: The label is a valid autoreleased AppKit view that the stack retains, the
        // fonts and colors are shared AppKit objects.
        unsafe {
            let label: *mut Object =
                msg_send![class!(NSTextField), wrappingLabelWithString: ns_string(text)];
            match style {
                Style::Title => {
                    let size: f64 = msg_send![class!(NSFont), systemFontSize];
                    let font: *mut Object = msg_send![class!(NSFont),
                        systemFontOfSize: size,
                        weight: NS_FONT_WEIGHT_SEMIBOLD
                    ];
                    let _: () = msg_send![label, setFont: font];
                }
                Style::Name => {
                    let size: f64 = msg_send![class!(NSFont), smallSystemFontSize];
                    let font: *mut Object = msg_send![class!(NSFont), systemFontOfSize: size];
                    let color: *mut Object = msg_send![class!(NSColor), secondaryLabelColor];
                    let _: () = msg_send![label, setFont: font];
                    let _: () = msg_send![label, setTextColor: color];
                }
                // Values can be copied, like a camera serial number or coordinates.
                Style::Value => {
                    let _: () = msg_send![label, setSelectable: Bool::YES];
                }
            }
            let _: () = msg_send![&*self.stack, addArrangedSubview: label];
            let width: *mut Object = msg_send![label, widthAnchor];
            let stack_width: *mut Object = msg_send![&*self.stack, widthAnchor];
            let constraint: *mut Object = msg_send![width, constraintEqualToAnchor: stack_width];
            let _: () = msg_send![constraint, setActive: Bool::YES];
            label
        }
    }
}

// How a label of the inspector looks.
#[derive(Clone, Copy)]
enum Style {
    Title,
    Name,
    Value,
}
