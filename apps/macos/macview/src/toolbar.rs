/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The editing toolbar, built from system symbol images.

use std::ptr::null_mut;

use macview_appkit::ns_string;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool, Sel};
use objc2::{class, msg_send, sel};

use crate::headers::*;

/// The identifier of the selection tools group.
pub(crate) const TOOLS_ITEM: &str = "tools";
/// The identifier of the item the adjustments popover is shown from.
pub(crate) const ADJUST_ITEM: &str = "adjust";
/// The identifier of the item the effects popover is shown from.
pub(crate) const EFFECTS_ITEM: &str = "effects";
/// The identifier of the item the straighten popover is shown from.
pub(crate) const STRAIGHTEN_ITEM: &str = "straighten";
/// The identifier of the compare item, whose symbol shows whether it is on.
pub(crate) const COMPARE_ITEM: &str = "compare";

// The symbol of the compare item, filled while the original is shown.
pub(crate) const COMPARE_SYMBOL: &str = "square.split.2x1";
pub(crate) const COMPARE_ON_SYMBOL: &str = "square.split.2x1.fill";

// The identifiers AppKit gives its standard spaces.
const SPACE_ITEM: &str = "NSToolbarSpaceItem";
const FLEXIBLE_SPACE_ITEM: &str = "NSToolbarFlexibleSpaceItem";

// A segment of a toolbar item group: its symbol and label.
type Segment = (&'static str, &'static str);

// What a toolbar item shows and does.
enum Kind {
    Button(&'static str),
    Group(i64, &'static [Segment]),
}

struct ItemSpec {
    identifier: &'static str,
    label: &'static str,
    tool_tip: &'static str,
    action: Sel,
    kind: Kind,
}

fn items() -> [ItemSpec; 9] {
    [
        ItemSpec {
            identifier: TOOLS_ITEM,
            label: "Tools",
            tool_tip: "",
            action: sel!(selectTool:),
            kind: Kind::Group(
                NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_SELECT_ONE,
                &[
                    ("hand.raised", "Move (V)"),
                    ("rectangle.dashed", "Rectangular Selection (M)"),
                    ("circle.dashed", "Elliptical Selection (E)"),
                ],
            ),
        },
        ItemSpec {
            identifier: "rotate",
            label: "Rotate",
            tool_tip: "",
            action: sel!(rotate:),
            kind: Kind::Group(
                NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_MOMENTARY,
                &[
                    ("rotate.left", "Rotate Left"),
                    ("rotate.right", "Rotate Right"),
                ],
            ),
        },
        ItemSpec {
            identifier: "flip",
            label: "Flip",
            tool_tip: "",
            action: sel!(flip:),
            kind: Kind::Group(
                NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_MOMENTARY,
                &[
                    (
                        "arrow.left.and.right.righttriangle.left.righttriangle.right",
                        "Flip Horizontal",
                    ),
                    (
                        "arrow.up.and.down.righttriangle.up.righttriangle.down",
                        "Flip Vertical",
                    ),
                ],
            ),
        },
        ItemSpec {
            identifier: "crop",
            label: "Crop",
            tool_tip: "Crop to Selection",
            action: sel!(cropToSelection:),
            kind: Kind::Button("crop"),
        },
        ItemSpec {
            identifier: STRAIGHTEN_ITEM,
            label: "Straighten",
            tool_tip: "Straighten",
            action: sel!(straighten:),
            kind: Kind::Button("level"),
        },
        ItemSpec {
            identifier: ADJUST_ITEM,
            label: "Adjust",
            tool_tip: "Adjust Light and Color",
            action: sel!(showAdjustments:),
            kind: Kind::Button("slider.horizontal.3"),
        },
        ItemSpec {
            identifier: EFFECTS_ITEM,
            label: "Effects",
            tool_tip: "Filters and Effects",
            action: sel!(showEffects:),
            kind: Kind::Button("camera.filters"),
        },
        ItemSpec {
            identifier: COMPARE_ITEM,
            label: "Compare",
            tool_tip: "Compare with Original",
            action: sel!(toggleCompare:),
            kind: Kind::Button(COMPARE_SYMBOL),
        },
        ItemSpec {
            identifier: "zoom",
            label: "Zoom",
            tool_tip: "",
            action: sel!(zoom:),
            kind: Kind::Group(
                NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_MOMENTARY,
                &[
                    ("minus.magnifyingglass", "Zoom Out"),
                    ("plus.magnifyingglass", "Zoom In"),
                ],
            ),
        },
    ]
}

const DEFAULT_ITEMS: [&str; 9] = [
    TOOLS_ITEM,
    SPACE_ITEM,
    "rotate",
    "flip",
    "crop",
    FLEXIBLE_SPACE_ITEM,
    ADJUST_ITEM,
    EFFECTS_ITEM,
    COMPARE_ITEM,
];

/// Returns an autoreleased system symbol image, described by `description` for accessibility.
pub(crate) fn symbol(name: &str, description: &str) -> *mut Object {
    // SAFETY: NSImage returns an autoreleased image, or null for an unknown symbol.
    unsafe {
        msg_send![class!(NSImage),
            imageWithSystemSymbolName: ns_string(name),
            accessibilityDescription: ns_string(description)
        ]
    }
}

/// Returns the identifiers of the items a new toolbar shows.
pub(crate) fn default_item_identifiers() -> *mut Object {
    ns_string_array(&DEFAULT_ITEMS)
}

/// Returns the identifiers of all items the toolbar can show.
pub(crate) fn allowed_item_identifiers() -> *mut Object {
    let mut identifiers: Vec<&str> = items().iter().map(|item| item.identifier).collect();
    identifiers.extend([SPACE_ITEM, FLEXIBLE_SPACE_ITEM]);
    ns_string_array(&identifiers)
}

/// Returns an autoreleased toolbar item for `identifier`, or null for an unknown one.
pub(crate) fn create_item(identifier: *mut Object) -> *mut Object {
    let identifier_string = crate::editing::rust_string(identifier);
    let Some(spec) = items()
        .into_iter()
        .find(|item| item.identifier == identifier_string)
    else {
        return null_mut();
    };
    // SAFETY: The toolbar item constructors return valid items, whose images and labels are
    // autoreleased objects they retain.
    unsafe {
        let item: *mut Object = match spec.kind {
            Kind::Button(name) => {
                let item: Allocated<Object> = msg_send![class!(NSToolbarItem), alloc];
                let item: Retained<Object> = msg_send![item, initWithItemIdentifier: identifier];
                let _: () = msg_send![&*item, setImage: symbol(name, spec.label)];
                let _: () = msg_send![&*item, setBordered: Bool::YES];
                let _: () = msg_send![&*item, setToolTip: ns_string(spec.tool_tip)];
                Retained::autorelease_ptr(item)
            }
            Kind::Group(mode, segments) => {
                let images: Vec<*mut Object> = segments
                    .iter()
                    .map(|(name, label)| symbol(name, label))
                    .collect();
                let images: *mut Object = ns_array(&images);
                let labels: Vec<&str> = segments.iter().map(|(_, label)| *label).collect();
                let group: *mut Object = msg_send![class!(NSToolbarItemGroup),
                    groupWithItemIdentifier: identifier,
                    images: images,
                    selectionMode: mode,
                    labels: ns_string_array(&labels),
                    target: null_mut::<Object>(),
                    action: spec.action
                ];
                let subitems: *mut Object = msg_send![group, subitems];
                for (index, (_, label)) in segments.iter().enumerate() {
                    let subitem: *mut Object = msg_send![subitems, objectAtIndex: index];
                    let _: () = msg_send![subitem, setToolTip: ns_string(label)];
                }
                group
            }
        };
        let _: () = msg_send![item, setLabel: ns_string(spec.label)];
        let _: () = msg_send![item, setPaletteLabel: ns_string(spec.label)];
        let _: () = msg_send![item, setAction: spec.action];
        item
    }
}

/// Creates an owned toolbar whose items `delegate` makes.
pub(crate) fn create_toolbar(delegate: *mut Object) -> Retained<Object> {
    // SAFETY: NSToolbar keeps a weak reference to its delegate, the window controller, which
    // lives as long as the window that holds the toolbar.
    unsafe {
        let toolbar: Allocated<Object> = msg_send![class!(NSToolbar), alloc];
        let toolbar: Retained<Object> =
            msg_send![toolbar, initWithIdentifier: ns_string!("MacViewEditingToolbar")];
        let _: () = msg_send![&*toolbar, setDelegate: delegate];
        let _: () = msg_send![&*toolbar, setDisplayMode: NS_TOOLBAR_DISPLAY_MODE_ICON_ONLY];
        let _: () = msg_send![&*toolbar, setAllowsUserCustomization: Bool::YES];
        let _: () = msg_send![&*toolbar, setAutosavesConfiguration: Bool::YES];
        toolbar
    }
}

/// Returns the item of `toolbar` with `identifier` while it is visible, or null.
///
/// # Safety
///
/// `toolbar` must be null or point to a live `NSToolbar`.
pub(crate) unsafe fn visible_item(toolbar: *mut Object, identifier: &str) -> *mut Object {
    if toolbar.is_null() {
        return null_mut();
    }
    // SAFETY: The caller supplies a live toolbar, which owns its items.
    unsafe {
        let visible: Bool = msg_send![toolbar, isVisible];
        if !visible.as_bool() {
            return null_mut();
        }
        let items: *mut Object = msg_send![toolbar, visibleItems];
        let count: usize = msg_send![items, count];
        (0..count)
            .map(|index| -> *mut Object { msg_send![items, objectAtIndex: index] })
            .find(|item| {
                let item_identifier: *mut Object = msg_send![*item, itemIdentifier];
                crate::editing::rust_string(item_identifier) == identifier
            })
            .unwrap_or(null_mut())
    }
}
