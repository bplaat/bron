/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The toolbars of an image window: NSToolbars unified with the titlebar, one to view the
//! image and one to edit it, which are swapped by their Edit and Done buttons. Related
//! buttons are one control with dividers, they run the same actions as the menu.

use std::ffi::{CStr, c_char};
use std::ptr::null_mut;

use imageview_core::appkit::ns_string;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send, sel};

use super::headers::*;
use crate::AppEvent;

const NS_TOOLBAR_DISPLAY_MODE_ICON_ONLY: u64 = 2;
const NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_SELECT_ONE: i64 = 0;
const NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_MOMENTARY: i64 = 2;
const NS_TOOLBAR_ITEM_GROUP_CONTROL_REPRESENTATION_EXPANDED: i64 = 1;
const NS_WINDOW_TOOLBAR_STYLE_UNIFIED: i64 = 3;

// A button with its tooltip, menu action and SF Symbols image.
struct Button {
    label: &'static str,
    action: &'static str,
    symbol: &'static str,
}

const fn button(label: &'static str, action: &'static str, symbol: &'static str) -> Button {
    Button {
        label,
        action,
        symbol,
    }
}

enum Item {
    // Related buttons shown as one control with dividers, a navigational one at the leading
    // edge before the title. A toggled group always shows one of its buttons as on. The symbol
    // is shown in the overflow menu of a narrow window.
    Group {
        label: &'static str,
        symbol: &'static str,
        buttons: &'static [Button],
        navigational: bool,
        toggled: bool,
    },
    // A button with an image that opens a menu, with separators between its sections.
    Menu {
        label: &'static str,
        symbol: &'static str,
        sections: &'static [&'static [Button]],
    },
    // A button with a title instead of an image.
    Text(Button),
    FlexibleSpace,
}

impl Item {
    // The buttons of the item, menu sections after each other.
    fn buttons(&self) -> Vec<&Button> {
        match self {
            Self::Group { buttons, .. } => buttons.iter().collect(),
            Self::Menu { sections, .. } => sections.iter().flat_map(|section| *section).collect(),
            Self::Text(button) => vec![button],
            Self::FlexibleSpace => Vec::new(),
        }
    }
}

/// Which toolbar a window shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    View,
    Edit,
}

impl Mode {
    const fn name(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Edit => "edit",
        }
    }

    const fn items(self) -> &'static [Item] {
        match self {
            Self::View => VIEW_ITEMS,
            Self::Edit => EDIT_ITEMS,
        }
    }

    // The identifier of an item, a prefix with the mode followed by the item index.
    fn identifier(self, index: usize) -> String {
        format!("imageview.toolbar.{}.{index}", self.name())
    }
}

const VIEW_ITEMS: &[Item] = &[
    Item::Group {
        label: "Navigate",
        symbol: "arrow.left.arrow.right",
        buttons: &[
            button("Previous Image", "view.previous", "chevron.left"),
            button("Next Image", "view.next", "chevron.right"),
        ],
        navigational: true,
        toggled: false,
    },
    Item::FlexibleSpace,
    Item::Group {
        label: "Zoom",
        symbol: "magnifyingglass",
        buttons: &[
            button("Zoom Out", "view.zoom_out", "minus.magnifyingglass"),
            button("Actual Size", "view.actual_size", "1.magnifyingglass"),
            button("Zoom In", "view.zoom_in", "plus.magnifyingglass"),
        ],
        navigational: false,
        toggled: false,
    },
    Item::Text(button("Edit", "edit.toggle", "pencil.tip.crop.circle")),
];

const EDIT_ITEMS: &[Item] = &[
    Item::Group {
        label: "History",
        symbol: "clock.arrow.circlepath",
        buttons: &[
            button("Undo", "edit.undo", "arrow.uturn.backward"),
            button("Redo", "edit.redo", "arrow.uturn.forward"),
        ],
        navigational: true,
        toggled: false,
    },
    Item::FlexibleSpace,
    Item::Group {
        label: "Select",
        symbol: "rectangle.dashed",
        buttons: &[
            button("Whole Image", "select.whole", "photo"),
            button(
                "Rectangle Selection",
                "select.rectangle",
                "rectangle.dashed",
            ),
            button("Ellipse Selection", "select.ellipse", "circle.dashed"),
        ],
        navigational: false,
        toggled: true,
    },
    Item::Group {
        label: "Transform",
        symbol: "rotate.right",
        buttons: &[
            button("Rotate Left", "image.rotate_left", "rotate.left"),
            button("Rotate Right", "image.rotate_right", "rotate.right"),
            button(
                "Flip Horizontal",
                "image.flip_horizontal",
                "arrow.left.and.right.righttriangle.left.righttriangle.right",
            ),
            button(
                "Flip Vertical",
                "image.flip_vertical",
                "arrow.up.and.down.righttriangle.up.righttriangle.down",
            ),
        ],
        navigational: false,
        toggled: false,
    },
    Item::Group {
        label: "Crop",
        symbol: "crop",
        buttons: &[button("Crop", "image.crop", "crop")],
        navigational: false,
        toggled: false,
    },
    Item::Group {
        label: "Resize",
        symbol: "arrow.up.left.and.arrow.down.right",
        buttons: &[button(
            "Resize",
            "image.resize",
            "arrow.up.left.and.arrow.down.right",
        )],
        navigational: false,
        toggled: false,
    },
    Item::Group {
        label: "Adjust",
        symbol: "slider.horizontal.3",
        buttons: &[button("Adjust", "filter.adjust", "slider.horizontal.3")],
        navigational: false,
        toggled: false,
    },
    Item::Menu {
        label: "Filters",
        symbol: "camera.filters",
        sections: &[
            &[button(
                "Auto Enhance",
                "filter.auto_enhance",
                "wand.and.stars",
            )],
            &[
                button("Grayscale", "filter.grayscale", "circle.lefthalf.filled"),
                button("Sepia", "filter.sepia", "camera.filters"),
                button("Invert", "filter.invert", "circle.righthalf.filled.inverse"),
                button(
                    "Threshold...",
                    "filter.threshold",
                    "circle.grid.2x1.left.filled",
                ),
                button("Solarize...", "filter.solarize", "sun.max"),
            ],
            &[
                button("Gaussian Blur...", "filter.gaussian_blur", "drop"),
                button("Box Blur...", "filter.box_blur", "square.dashed"),
                button("Motion Blur...", "filter.motion_blur", "wind"),
                button(
                    "Radial Blur...",
                    "filter.radial_blur",
                    "smallcircle.filled.circle",
                ),
            ],
            &[
                button("Sharpen...", "filter.sharpen", "wand.and.rays"),
                button("Reduce Noise...", "filter.reduce_noise", "aqi.medium"),
                button("Pixelate...", "filter.pixelate", "square.grid.3x3"),
            ],
        ],
    },
    Item::Text(button("Done", "edit.toggle", "checkmark.circle")),
];

define_class!(
    // Creates the toolbar items of a mode and runs their actions, the toolbar does not
    // retain it.
    #[unsafe(super(NSObject))]
    #[name = "ImageViewToolbarDelegate"]
    #[ivars = Mode]
    struct ToolbarDelegate;

    impl ToolbarDelegate {
        #[unsafe(method(toolbarDefaultItemIdentifiers:))]
        fn _default_item_identifiers(&self, _: *mut Object) -> *mut Object {
            item_identifiers(*self.ivars())
        }

        #[unsafe(method(toolbarAllowedItemIdentifiers:))]
        fn _allowed_item_identifiers(&self, _: *mut Object) -> *mut Object {
            item_identifiers(*self.ivars())
        }

        // The method does not own its result, so the item is autoreleased.
        #[unsafe(method(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn _item(&self, _: *mut Object, identifier: *mut Object, _: Bool) -> *mut Object {
            self.item(identifier)
                .map_or(null_mut(), Retained::autorelease_ptr)
        }

        #[unsafe(method(itemSelected:))]
        fn _item_selected(&self, sender: *mut Object) {
            // SAFETY: The sender is a toolbar item group.
            let (identifier, index): (*mut Object, isize) =
                unsafe { (msg_send![sender, itemIdentifier], msg_send![sender, selectedIndex]) };
            self.run_action(identifier, index);
        }

        #[unsafe(method(buttonSelected:))]
        fn _button_selected(&self, sender: *mut Object) {
            // SAFETY: The sender is a toolbar item.
            let identifier: *mut Object = unsafe { msg_send![sender, itemIdentifier] };
            self.run_action(identifier, 0);
        }

        #[unsafe(method(menuItemSelected:))]
        fn _menu_item_selected(&self, sender: *mut Object) {
            // SAFETY: The sender is a menu item of a group or menu.
            let (identifier, index): (*mut Object, isize) =
                unsafe { (msg_send![sender, representedObject], msg_send![sender, tag]) };
            self.run_action(identifier, index);
        }
    }
);

impl ToolbarDelegate {
    fn new(mode: Mode) -> Retained<Self> {
        // SAFETY: NSObject's `init` returns the initialized delegate.
        unsafe {
            let delegate: Allocated<Self> = msg_send![Self::class(), alloc];
            msg_send![super(delegate.set_ivars(mode)), init]
        }
    }

    // The item of an identifier of this delegate's mode.
    fn item_at(&self, identifier: *mut Object) -> Option<&'static Item> {
        let mode = *self.ivars();
        let identifier = string(identifier)?;
        let index = (0..mode.items().len()).find(|index| mode.identifier(*index) == identifier)?;
        mode.items().get(index)
    }

    fn item(&self, identifier: *mut Object) -> Option<Retained<Object>> {
        let target = self as *const Self as *mut Object;
        // SAFETY: All objects are valid AppKit instances, the toolbar retains the items.
        unsafe {
            let item: Retained<Object> = match self.item_at(identifier)? {
                Item::Group {
                    label,
                    symbol: group_symbol,
                    buttons,
                    navigational,
                    toggled,
                } => {
                    let images: *mut Object =
                        msg_send![class!(NSMutableArray), arrayWithCapacity: buttons.len()];
                    // The segments are as wide as their labels, also when only icons are
                    // shown, so they get none, the icon descriptions and tooltips name them.
                    let labels: *mut Object =
                        msg_send![class!(NSMutableArray), arrayWithCapacity: buttons.len()];
                    for button in *buttons {
                        let _: () =
                            msg_send![images, addObject: symbol(button.symbol, button.label)];
                        let _: () = msg_send![labels, addObject: ns_string("")];
                    }
                    let group: Retained<Object> = msg_send![class!(NSToolbarItemGroup),
                        groupWithItemIdentifier: identifier,
                        images: images,
                        selectionMode: if *toggled {
                            NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_SELECT_ONE
                        } else {
                            NS_TOOLBAR_ITEM_GROUP_SELECTION_MODE_MOMENTARY
                        },
                        labels: labels,
                        target: target,
                        action: sel!(itemSelected:)
                    ];
                    let subitems: *mut Object = msg_send![&*group, subitems];
                    for (index, button) in buttons.iter().enumerate() {
                        let subitem: *mut Object = msg_send![subitems, objectAtIndex: index];
                        let _: () = msg_send![subitem, setToolTip: ns_string(button.label)];
                    }
                    // Editing starts with the first button on, like the whole image.
                    if *toggled {
                        let _: () = msg_send![&*group, setSelectedIndex: 0isize];
                    }
                    // Navigational items are available since macOS 14.
                    let navigable: Bool =
                        msg_send![&*group, respondsToSelector: sel!(setNavigational:)];
                    if *navigational && navigable.as_bool() {
                        let _: () = msg_send![&*group, setNavigational: Bool::YES];
                    }
                    // Narrow windows would collapse the buttons into a pop-up without an
                    // image, a group that does not fit moves to the overflow menu instead.
                    let _: () = msg_send![&*group,
                        setControlRepresentation: NS_TOOLBAR_ITEM_GROUP_CONTROL_REPRESENTATION_EXPANDED
                    ];
                    let _: () = msg_send![&*group, setLabel: ns_string(label)];
                    let overflow = overflow_item(label, group_symbol, buttons, identifier, target);
                    let _: () = msg_send![&*group, setMenuFormRepresentation: &*overflow];
                    let _: () = msg_send![&*group, setTarget: target];
                    let _: () = msg_send![&*group, setAction: sel!(itemSelected:)];
                    group
                }
                Item::Menu {
                    label,
                    symbol: image,
                    sections,
                } => {
                    let item: Allocated<Object> = msg_send![class!(NSMenuToolbarItem), alloc];
                    let item: Retained<Object> =
                        msg_send![item, initWithItemIdentifier: identifier];
                    let items = menu(label, sections, identifier, target);
                    let _: () = msg_send![&*item, setMenu: &*items];
                    let _: () = msg_send![&*item, setImage: symbol(image, label)];
                    // A menu has one place, so the overflow entry gets its own.
                    let overflow =
                        submenu_item(label, image, &menu(label, sections, identifier, target));
                    let _: () = msg_send![&*item, setMenuFormRepresentation: &*overflow];
                    let _: () = msg_send![&*item, setLabel: ns_string(label)];
                    let _: () = msg_send![&*item, setToolTip: ns_string(label)];
                    item
                }
                Item::Text(button) => {
                    let item: Allocated<Object> = msg_send![class!(NSToolbarItem), alloc];
                    let item: Retained<Object> =
                        msg_send![item, initWithItemIdentifier: identifier];
                    let _: () = msg_send![&*item, setTitle: ns_string(button.label)];
                    let _: () = msg_send![&*item, setLabel: ns_string(button.label)];
                    let _: () = msg_send![&*item, setBordered: Bool::YES];
                    let _: () = msg_send![&*item, setTarget: target];
                    let _: () = msg_send![&*item, setAction: sel!(buttonSelected:)];
                    let overflow = entry(button, 0, identifier, target);
                    let _: () = msg_send![&*item, setMenuFormRepresentation: &*overflow];
                    item
                }
                Item::FlexibleSpace => return None,
            };
            let label: *mut Object = msg_send![&*item, label];
            let _: () = msg_send![&*item, setPaletteLabel: label];
            Some(item)
        }
    }

    // Runs the action of a button of the item of an identifier.
    fn run_action(&self, identifier: *mut Object, index: isize) {
        let Some(item) = self.item_at(identifier) else {
            return;
        };
        if let Some(button) = usize::try_from(index)
            .ok()
            .and_then(|index| item.buttons().get(index).copied())
        {
            super::send(AppEvent::Action(button.action));
        }
    }
}

// The string of an NSString, which lives in the current pool.
fn string<'a>(string: *mut Object) -> Option<&'a str> {
    // SAFETY: AppKit passes a valid NSString, its UTF-8 representation lives in this pool.
    unsafe {
        let bytes: *const c_char = msg_send![string, UTF8String];
        if bytes.is_null() {
            return None;
        }
        CStr::from_ptr(bytes).to_str().ok()
    }
}

// The autoreleased identifiers of all items of a mode.
fn item_identifiers(mode: Mode) -> *mut Object {
    // SAFETY: The array and strings are autoreleased, the flexible space identifier is a
    // constant of AppKit.
    unsafe {
        let identifiers: *mut Object =
            msg_send![class!(NSMutableArray), arrayWithCapacity: mode.items().len()];
        for (index, item) in mode.items().iter().enumerate() {
            let identifier = match item {
                Item::FlexibleSpace => NSToolbarFlexibleSpaceItemIdentifier,
                _ => ns_string(&mode.identifier(index)),
            };
            let _: () = msg_send![identifiers, addObject: identifier];
        }
        identifiers
    }
}

// A menu of buttons with separators between the sections, which run the actions of the item
// of an identifier.
fn menu(
    label: &str,
    sections: &[&[Button]],
    identifier: *mut Object,
    target: *mut Object,
) -> Retained<Object> {
    // SAFETY: All objects are valid AppKit instances, the menu retains its items.
    unsafe {
        let menu: Allocated<Object> = msg_send![class!(NSMenu), alloc];
        let menu: Retained<Object> = msg_send![menu, initWithTitle: ns_string(label)];
        let mut index: isize = 0;
        for (section_index, section) in sections.iter().enumerate() {
            if section_index > 0 {
                let separator: *mut Object = msg_send![class!(NSMenuItem), separatorItem];
                let _: () = msg_send![&*menu, addItem: separator];
            }
            for button in *section {
                let _: () = msg_send![&*menu, addItem: &*entry(button, index, identifier, target)];
                index += 1;
            }
        }
        menu
    }
}

// A menu item that runs the action of the button at an index of the item of an identifier.
fn entry(
    button: &Button,
    index: isize,
    identifier: *mut Object,
    target: *mut Object,
) -> Retained<Object> {
    // SAFETY: All objects are valid AppKit instances, the item sends its action to the target.
    unsafe {
        let entry: Allocated<Object> = msg_send![class!(NSMenuItem), alloc];
        let entry: Retained<Object> = msg_send![entry,
            initWithTitle: ns_string(button.label),
            action: sel!(menuItemSelected:),
            keyEquivalent: ns_string("")
        ];
        let _: () = msg_send![&*entry, setImage: symbol(button.symbol, button.label)];
        let _: () = msg_send![&*entry, setTarget: target];
        let _: () = msg_send![&*entry, setRepresentedObject: identifier];
        let _: () = msg_send![&*entry, setTag: index];
        entry
    }
}

// The overflow menu entry of a group: the item of its only button, or a submenu with the
// labels its segments do not show.
fn overflow_item(
    label: &str,
    image: &str,
    buttons: &[Button],
    identifier: *mut Object,
    target: *mut Object,
) -> Retained<Object> {
    if let [button] = buttons {
        return entry(button, 0, identifier, target);
    }
    submenu_item(label, image, &menu(label, &[buttons], identifier, target))
}

// A menu item with an image that opens a submenu.
fn submenu_item(label: &str, image: &str, menu: &Object) -> Retained<Object> {
    // SAFETY: The item retains its submenu and autoreleased image.
    unsafe {
        let item: Retained<Object> = msg_send![class!(NSMenuItem), new];
        let _: () = msg_send![&*item, setTitle: ns_string(label)];
        let _: () = msg_send![&*item, setImage: symbol(image, label)];
        let _: () = msg_send![&*item, setSubmenu: menu];
        item
    }
}

// The autoreleased SF Symbols image, described by a label.
fn symbol(name: &str, label: &str) -> *mut Object {
    // SAFETY: The strings are autoreleased and NSImage returns an autoreleased image.
    unsafe {
        msg_send![class!(NSImage),
            imageWithSystemSymbolName: ns_string(name),
            accessibilityDescription: ns_string(label)
        ]
    }
}

/// The view and edit toolbars of a window with their delegates, which the toolbars do not
/// retain.
pub(super) struct Toolbars {
    view: Retained<Object>,
    edit: Retained<Object>,
    _delegates: [Retained<ToolbarDelegate>; 2],
}

impl Toolbars {
    /// Shows the view toolbar in the window, unified with its titlebar.
    pub(super) fn install(window: *mut Object) -> Self {
        let delegates = [Mode::View, Mode::Edit].map(ToolbarDelegate::new);
        let [view, edit] = [0, 1].map(|index| toolbar(&delegates[index]));
        // SAFETY: The window is a live NSWindow that retains its toolbar.
        unsafe {
            let _: () = msg_send![window, setToolbar: &*view];
            let _: () = msg_send![window, setToolbarStyle: NS_WINDOW_TOOLBAR_STYLE_UNIFIED];
        }
        Self {
            view,
            edit,
            _delegates: delegates,
        }
    }

    /// Shows the toolbar of a mode in the window.
    pub(super) fn show(&self, window: *mut Object, mode: Mode) {
        let toolbar = match mode {
            Mode::View => &self.view,
            Mode::Edit => &self.edit,
        };
        // SAFETY: The window is a live NSWindow that retains its toolbar.
        unsafe {
            let _: () = msg_send![window, setToolbar: &**toolbar];
        }
    }

    /// Shows the button of a toggled group of the edit toolbar with an action as on.
    pub(super) fn set_toggled(&self, action: &str) {
        for (index, item) in EDIT_ITEMS.iter().enumerate() {
            let Item::Group {
                buttons,
                toggled: true,
                ..
            } = item
            else {
                continue;
            };
            let Some(on) = buttons.iter().position(|button| button.action == action) else {
                continue;
            };
            if let Some(group) = self.edit_item_at(index) {
                // SAFETY: The group has a segment for every button.
                unsafe {
                    let _: () = msg_send![group, setSelectedIndex: on as isize];
                }
            }
        }
    }

    /// Enables the Edit button of the view toolbar.
    pub(super) fn set_editable(&self, editable: bool) {
        let Some(index) = VIEW_ITEMS.iter().position(|item| {
            item.buttons()
                .iter()
                .any(|button| button.action == "edit.toggle")
        }) else {
            return;
        };
        if let Some(item) = self.item_at(Mode::View, index) {
            // SAFETY: The item and its overflow entry are live AppKit objects.
            unsafe {
                let _: () = msg_send![item, setAutovalidates: Bool::NO];
                let _: () = msg_send![item, setEnabled: objc_bool(editable)];
            }
        }
    }

    /// The edit toolbar item with a button that runs an action, to show a popover at.
    pub(super) fn edit_item(&self, action: &str) -> Option<*mut Object> {
        let index = EDIT_ITEMS
            .iter()
            .position(|item| item.buttons().iter().any(|button| button.action == action))?;
        self.edit_item_at(index)
    }

    // The edit toolbar item at an index of the items.
    fn edit_item_at(&self, index: usize) -> Option<*mut Object> {
        self.item_at(Mode::Edit, index)
    }

    // The item at an index of the items of the toolbar of a mode.
    fn item_at(&self, mode: Mode, index: usize) -> Option<*mut Object> {
        let identifier = mode.identifier(index);
        let toolbar = match mode {
            Mode::View => &self.view,
            Mode::Edit => &self.edit,
        };
        // SAFETY: The toolbar keeps its items in an NSArray.
        unsafe {
            let items: *mut Object = msg_send![&**toolbar, items];
            let count: usize = msg_send![items, count];
            (0..count)
                .map(|index| -> *mut Object { msg_send![items, objectAtIndex: index] })
                .find(|item| {
                    let item_identifier: *mut Object = msg_send![*item, itemIdentifier];
                    string(item_identifier) == Some(identifier.as_str())
                })
        }
    }
}

impl Drop for Toolbars {
    fn drop(&mut self) {
        // SAFETY: The toolbars are live NSToolbars. Their delegates are not retained and the
        // window can keep a toolbar after this, so it forgets its delegate first.
        unsafe {
            for toolbar in [&self.view, &self.edit] {
                let _: () = msg_send![&**toolbar, setDelegate: null_mut::<Object>()];
            }
        }
    }
}

fn toolbar(delegate: &ToolbarDelegate) -> Retained<Object> {
    let mode = *delegate.ivars();
    // SAFETY: The toolbar does not retain its delegate, which `Toolbars` keeps.
    unsafe {
        let toolbar: Allocated<Object> = msg_send![class!(NSToolbar), alloc];
        let toolbar: Retained<Object> = msg_send![toolbar,
            initWithIdentifier: ns_string(&format!("imageview.toolbar.{}", mode.name()))
        ];
        let _: () = msg_send![&*toolbar, setDelegate: delegate];
        let _: () = msg_send![&*toolbar, setDisplayMode: NS_TOOLBAR_DISPLAY_MODE_ICON_ONLY];
        let _: () = msg_send![&*toolbar, setAllowsUserCustomization: Bool::NO];
        // Without customization, also remove the display mode menu of a right click, which
        // macOS 15 and later can disable.
        let customizable: Bool =
            msg_send![&*toolbar, respondsToSelector: sel!(setAllowsDisplayModeCustomization:)];
        if customizable.as_bool() {
            let _: () = msg_send![&*toolbar, setAllowsDisplayModeCustomization: Bool::NO];
        }
        toolbar
    }
}
