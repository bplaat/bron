/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The toolbar of an image window: an NSToolbar unified with the titlebar, with the previous
//! and next image buttons as one control at the leading edge and the zoom commands as one
//! control at the trailing edge, which run the same actions as the menu.

use std::ffi::{CStr, c_char};

use imageview_core::appkit::ns_string;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use crate::AppEvent;

// The prefix of the item identifiers, followed by the item index.
const IDENTIFIER_PREFIX: &str = "imageview.toolbar.";
const NS_TOOLBAR_DISPLAY_MODE_ICON_ONLY: u64 = 2;
const NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_MOMENTARY: i64 = 2;
const NS_WINDOW_TOOLBAR_STYLE_UNIFIED: i64 = 3;

// A button with its tooltip, menu action and SF Symbols image.
struct Button {
    label: &'static str,
    action: &'static str,
    symbol: &'static str,
}

enum Item {
    // Related buttons shown as one control with dividers, a navigational one at the leading
    // edge before the title.
    Group {
        label: &'static str,
        buttons: &'static [Button],
        navigational: bool,
    },
    FlexibleSpace,
}

const ITEMS: &[Item] = &[
    Item::Group {
        label: "Navigate",
        buttons: &[
            Button {
                label: "Previous Image",
                action: "view.previous",
                symbol: "chevron.left",
            },
            Button {
                label: "Next Image",
                action: "view.next",
                symbol: "chevron.right",
            },
        ],
        navigational: true,
    },
    Item::FlexibleSpace,
    Item::Group {
        label: "Zoom",
        buttons: &[
            Button {
                label: "Zoom Out",
                action: "view.zoom_out",
                symbol: "minus.magnifyingglass",
            },
            Button {
                label: "Actual Size",
                action: "view.actual_size",
                symbol: "1.magnifyingglass",
            },
            Button {
                label: "Zoom In",
                action: "view.zoom_in",
                symbol: "plus.magnifyingglass",
            },
        ],
        navigational: false,
    },
];

define_class!(
    // Creates the toolbar items and runs their actions, the toolbar does not retain it.
    #[unsafe(super(NSObject))]
    #[name = "ImageViewToolbarDelegate"]
    struct ToolbarDelegate;

    impl ToolbarDelegate {
        #[unsafe(method(toolbarDefaultItemIdentifiers:))]
        fn _default_item_identifiers(&self, _: *mut Object) -> *mut Object {
            item_identifiers()
        }

        #[unsafe(method(toolbarAllowedItemIdentifiers:))]
        fn _allowed_item_identifiers(&self, _: *mut Object) -> *mut Object {
            item_identifiers()
        }

        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn _item(
            &self,
            _: *mut Object,
            identifier: *mut Object,
            _: Bool,
        ) -> Option<Retained<Object>> {
            self.item(identifier)
        }

        #[unsafe(method(itemSelected:))]
        fn _item_selected(&self, sender: *mut Object) {
            // SAFETY: The sender is a toolbar item group.
            let (identifier, index): (*mut Object, isize) =
                unsafe { (msg_send![sender, itemIdentifier], msg_send![sender, selectedIndex]) };
            run_action(identifier, index);
        }

        #[unsafe(method(menuItemSelected:))]
        fn _menu_item_selected(&self, sender: *mut Object) {
            // SAFETY: The sender is an overflow menu item of a group.
            let (identifier, index): (*mut Object, isize) =
                unsafe { (msg_send![sender, representedObject], msg_send![sender, tag]) };
            run_action(identifier, index);
        }
    }
);

impl ToolbarDelegate {
    fn item(&self, identifier: *mut Object) -> Option<Retained<Object>> {
        let index = item_index(identifier)?;
        let target = self as *const Self as *mut Object;
        // SAFETY: All objects are valid AppKit instances, the toolbar retains the items.
        unsafe {
            let item: Retained<Object> = match ITEMS.get(index)? {
                Item::Group {
                    label,
                    buttons,
                    navigational,
                } => {
                    let images: *mut Object =
                        msg_send![class!(NSMutableArray), arrayWithCapacity: buttons.len()];
                    // The segments are as wide as their labels, also when only icons are
                    // shown, so they get none, the icon descriptions and tooltips name them.
                    let labels: *mut Object =
                        msg_send![class!(NSMutableArray), arrayWithCapacity: buttons.len()];
                    for button in *buttons {
                        let _: () = msg_send![images, addObject: symbol(button)];
                        let _: () = msg_send![labels, addObject: ns_string("")];
                    }
                    let group: Retained<Object> = msg_send![class!(NSToolbarItemGroup),
                        groupWithItemIdentifier: identifier,
                        images: images,
                        selectionMode: NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_MOMENTARY,
                        labels: labels,
                        target: target,
                        action: sel!(itemSelected:)
                    ];
                    let subitems: *mut Object = msg_send![&*group, subitems];
                    for (index, button) in buttons.iter().enumerate() {
                        let subitem: *mut Object = msg_send![subitems, objectAtIndex: index];
                        let _: () = msg_send![subitem, setToolTip: ns_string(button.label)];
                    }
                    // Navigational items are available since macOS 14.
                    let navigable: Bool =
                        msg_send![&*group, respondsToSelector: sel!(setNavigational:)];
                    if *navigational && navigable.as_bool() {
                        let _: () = msg_send![&*group, setNavigational: Bool::YES];
                    }
                    let _: () = msg_send![&*group, setLabel: ns_string(label)];
                    let _: () = msg_send![&*group,
                        setMenuFormRepresentation: &*overflow_menu_item(identifier, label, buttons, target)
                    ];
                    group
                }
                Item::FlexibleSpace => return None,
            };
            let label: *mut Object = msg_send![&*item, label];
            let _: () = msg_send![&*item, setPaletteLabel: label];
            let _: () = msg_send![&*item, setTarget: target];
            let _: () = msg_send![&*item, setAction: sel!(itemSelected:)];
            Some(item)
        }
    }
}

// The autoreleased identifiers of all items.
fn item_identifiers() -> *mut Object {
    // SAFETY: The array and strings are autoreleased, the flexible space identifier is a
    // constant of AppKit.
    unsafe {
        let identifiers: *mut Object =
            msg_send![class!(NSMutableArray), arrayWithCapacity: ITEMS.len()];
        for (index, item) in ITEMS.iter().enumerate() {
            let identifier = match item {
                Item::FlexibleSpace => NSToolbarFlexibleSpaceItemIdentifier,
                _ => ns_string(&format!("{IDENTIFIER_PREFIX}{index}")),
            };
            let _: () = msg_send![identifiers, addObject: identifier];
        }
        identifiers
    }
}

// The index of the item of an identifier.
fn item_index(identifier: *mut Object) -> Option<usize> {
    // SAFETY: AppKit passes a valid NSString, its UTF-8 representation lives in this pool.
    let identifier = unsafe {
        let bytes: *const c_char = msg_send![identifier, UTF8String];
        if bytes.is_null() {
            return None;
        }
        CStr::from_ptr(bytes).to_str().ok()?
    };
    identifier.strip_prefix(IDENTIFIER_PREFIX)?.parse().ok()
}

// The overflow menu entry of a group: a submenu with the labels its segments do not show.
unsafe fn overflow_menu_item(
    identifier: *mut Object,
    label: &str,
    buttons: &[Button],
    target: *mut Object,
) -> Retained<Object> {
    // SAFETY: All objects are valid AppKit instances, the menu retains its items.
    unsafe {
        let menu: Allocated<Object> = msg_send![class!(NSMenu), alloc];
        let menu: Retained<Object> = msg_send![menu, initWithTitle: ns_string(label)];
        for (index, button) in buttons.iter().enumerate() {
            let entry: Allocated<Object> = msg_send![class!(NSMenuItem), alloc];
            let entry: Retained<Object> = msg_send![entry,
                initWithTitle: ns_string(button.label),
                action: sel!(menuItemSelected:),
                keyEquivalent: ns_string("")
            ];
            let _: () = msg_send![&*entry, setImage: symbol(button)];
            let _: () = msg_send![&*entry, setTarget: target];
            let _: () = msg_send![&*entry, setRepresentedObject: identifier];
            let _: () = msg_send![&*entry, setTag: index as isize];
            let _: () = msg_send![&*menu, addItem: &*entry];
        }
        let item: Retained<Object> = msg_send![class!(NSMenuItem), new];
        let _: () = msg_send![&*item, setTitle: ns_string(label)];
        let _: () = msg_send![&*item, setSubmenu: &*menu];
        item
    }
}

// Runs the action of a button of the group of an item identifier.
fn run_action(identifier: *mut Object, index: isize) {
    let Some(Item::Group { buttons, .. }) = item_index(identifier).and_then(|item| ITEMS.get(item))
    else {
        return;
    };
    if let Some(button) = usize::try_from(index)
        .ok()
        .and_then(|index| buttons.get(index))
    {
        super::send(AppEvent::Action(button.action));
    }
}

// The autoreleased SF Symbols image of a button, described by its label.
fn symbol(button: &Button) -> *mut Object {
    // SAFETY: The strings are autoreleased and NSImage returns an autoreleased image.
    unsafe {
        msg_send![class!(NSImage),
            imageWithSystemSymbolName: ns_string(button.symbol),
            accessibilityDescription: ns_string(button.label)
        ]
    }
}

/// Shows the toolbar in the window, unified with its titlebar. Returns its delegate, which the
/// caller keeps alive while the window is open.
pub(super) fn install_toolbar(window: *mut Object) -> Retained<Object> {
    // SAFETY: The window is a live NSWindow that retains the toolbar.
    unsafe {
        let delegate: Retained<Object> = msg_send![ToolbarDelegate::class(), new];
        let toolbar: Allocated<Object> = msg_send![class!(NSToolbar), alloc];
        let toolbar: Retained<Object> =
            msg_send![toolbar, initWithIdentifier: ns_string("imageview.toolbar")];
        let _: () = msg_send![&*toolbar, setDelegate: &*delegate];
        let _: () = msg_send![&*toolbar, setDisplayMode: NS_TOOLBAR_DISPLAY_MODE_ICON_ONLY];
        let _: () = msg_send![&*toolbar, setAllowsUserCustomization: Bool::NO];
        // Without customization, also remove the display mode menu of a right click, which
        // macOS 15 and later can disable.
        let customizable: Bool =
            msg_send![&*toolbar, respondsToSelector: sel!(setAllowsDisplayModeCustomization:)];
        if customizable.as_bool() {
            let _: () = msg_send![&*toolbar, setAllowsDisplayModeCustomization: Bool::NO];
        }
        let _: () = msg_send![window, setToolbar: &*toolbar];
        let _: () = msg_send![window, setToolbarStyle: NS_WINDOW_TOOLBAR_STYLE_UNIFIED];
        delegate
    }
}
