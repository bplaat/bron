/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native header bar or inline toolbar controls, one set to view the image and one to edit it,
//! which are swapped by their Edit and Done buttons. They run the same actions as the menu.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_void};
use std::ptr::{from_ref, null};

use bwindow::EventLoopProxy;
use imageview_core::Media;

use super::headers::*;
use crate::AppEvent;

thread_local! {
    static PROXY: RefCell<Option<EventLoopProxy<AppEvent>>> = const { RefCell::new(None) };
    // Whether a toggled button is set by the application instead of the user.
    static SETTING: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn init(proxy: EventLoopProxy<AppEvent>) {
    PROXY.with_borrow_mut(|stored| *stored = Some(proxy));
}

/// Sends an event to the application.
pub(super) fn send(event: AppEvent) {
    PROXY.with_borrow(|proxy| {
        if let Some(proxy) = proxy {
            _ = proxy.send_user_event(event);
        }
    });
}

// A button with its tooltip, icon and menu action, without an icon its label is shown instead.
struct Button {
    label: &'static CStr,
    icon: &'static CStr,
    action: &'static str,
}

const fn icon(label: &'static CStr, icon: &'static CStr, action: &'static str) -> Button {
    Button {
        label,
        icon,
        action,
    }
}

const fn text(label: &'static CStr, action: &'static str) -> Button {
    icon(label, c"", action)
}

enum Item {
    // Related buttons shown as one control.
    Group(&'static [Button]),
    // Related buttons of which one is always on.
    Toggled(&'static [Button]),
    // A button with a label that opens a menu, with separators between its sections.
    Menu(&'static CStr, &'static [&'static [Button]]),
    // A button that stays on while what it shows is open, like a sidebar.
    Switch(Button),
}

// The items at the end are packed from the outer edge.
const VIEW_START: &[Item] = &[
    Item::Switch(icon(
        c"Show Sidebar",
        c"sidebar-show-symbolic",
        "view.sidebar",
    )),
    Item::Group(&[
        icon(c"Previous Image", c"go-previous-symbolic", "view.previous"),
        icon(c"Next Image", c"go-next-symbolic", "view.next"),
    ]),
];
const VIEW_END: &[Item] = &[
    Item::Group(&[text(c"Edit", "edit.toggle")]),
    Item::Switch(icon(
        c"Show Info",
        c"dialog-information-symbolic",
        "view.info",
    )),
    Item::Group(&[
        icon(c"Zoom Out", c"zoom-out-symbolic", "view.zoom_out"),
        icon(
            c"Actual Size",
            c"zoom-original-symbolic",
            "view.actual_size",
        ),
        icon(c"Zoom In", c"zoom-in-symbolic", "view.zoom_in"),
    ]),
];
const EDIT_START: &[Item] = &[
    Item::Group(&[
        icon(c"Undo", c"edit-undo-symbolic", "edit.undo"),
        icon(c"Redo", c"edit-redo-symbolic", "edit.redo"),
    ]),
    Item::Toggled(&[
        icon(c"Whole Image", c"image-x-generic-symbolic", "select.whole"),
        icon(
            c"Rectangle Selection",
            c"media-playback-stop-symbolic",
            "select.rectangle",
        ),
        icon(
            c"Ellipse Selection",
            c"media-record-symbolic",
            "select.ellipse",
        ),
    ]),
    Item::Group(&[
        icon(
            c"Rotate Left",
            c"object-rotate-left-symbolic",
            "image.rotate_left",
        ),
        icon(
            c"Rotate Right",
            c"object-rotate-right-symbolic",
            "image.rotate_right",
        ),
        icon(
            c"Flip Horizontal",
            c"object-flip-horizontal-symbolic",
            "image.flip_horizontal",
        ),
        icon(
            c"Flip Vertical",
            c"object-flip-vertical-symbolic",
            "image.flip_vertical",
        ),
    ]),
];
const EDIT_END: &[Item] = &[
    Item::Group(&[text(c"Done", "edit.toggle")]),
    Item::Menu(
        c"Filters",
        &[
            &[text(c"Auto Enhance", "filter.auto_enhance")],
            &[
                text(c"Grayscale", "filter.grayscale"),
                text(c"Sepia", "filter.sepia"),
                text(c"Invert", "filter.invert"),
                text(c"Threshold...", "filter.threshold"),
                text(c"Solarize...", "filter.solarize"),
            ],
            &[
                text(c"Gaussian Blur...", "filter.gaussian_blur"),
                text(c"Box Blur...", "filter.box_blur"),
                text(c"Motion Blur...", "filter.motion_blur"),
                text(c"Radial Blur...", "filter.radial_blur"),
            ],
            &[
                text(c"Sharpen...", "filter.sharpen"),
                text(c"Reduce Noise...", "filter.reduce_noise"),
                text(c"Pixelate...", "filter.pixelate"),
            ],
        ],
    ),
    Item::Group(&[
        text(c"Crop", "image.crop"),
        text(c"Resize", "image.resize"),
        text(c"Adjust", "filter.adjust"),
    ]),
];

extern "C" fn clicked(_: *mut c_void, button: &Button) {
    send(AppEvent::Action(button.action));
}

// Runs the action of a switch the user turned on or off.
extern "C" fn switched(_: *mut c_void, button: &Button) {
    if !SETTING.get() {
        send(AppEvent::Action(button.action));
    }
}

// Runs the action of a toggled button the user turned on, its group turns the others off.
extern "C" fn toggled(widget: *mut c_void, button: &Button) {
    // SAFETY: GTK passes the live toggle button.
    if !SETTING.get() && unsafe { gtk_toggle_button_get_active(widget) } != 0 {
        send(AppEvent::Action(button.action));
    }
}

/// Prefer integrated titlebars only on desktops that conventionally use them.
/// XDG_CURRENT_DESKTOP is a colon-separated list, e.g. Ubuntu sets ubuntu:GNOME.
/// Stop at the first recognized desktop so compatibility names do not override it.
fn desktop_uses_header_bar(desktop: &str) -> bool {
    for name in desktop.split(':').map(str::trim) {
        if ["GNOME", "GNOME-Classic", "Budgie", "Pantheon"]
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
        {
            return true;
        }
        if [
            "KDE",
            "Plasma",
            "MATE",
            "X-Cinnamon",
            "Cinnamon",
            "XFCE",
            "LXDE",
            "LXQt",
            "Unity",
            "GNOME-Flashback",
        ]
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
        {
            return false;
        }
    }
    false
}

pub(crate) fn uses_header_bar() -> bool {
    desktop_uses_header_bar(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
}

// Connects a signal of a widget to the action of a static button.
unsafe fn connect(widget: *mut c_void, signal: &CStr, handler: *const c_void, button: &Button) {
    // SAFETY: The handlers take the static button description, which outlives the widget.
    unsafe {
        g_signal_connect_data(
            widget,
            signal.as_ptr(),
            handler,
            from_ref(button).cast(),
            null(),
            0,
        );
    }
}

// The button of a description, with its icon or label.
unsafe fn new_button(button: &Button) -> *mut c_void {
    // SAFETY: GTK copies the strings.
    unsafe {
        if button.icon.is_empty() {
            gtk_button_new_with_label(button.label.as_ptr())
        } else {
            let widget = gtk_button_new_from_icon_name(button.icon.as_ptr(), GTK_ICON_SIZE_BUTTON);
            gtk_widget_set_tooltip_text(widget, button.label.as_ptr());
            widget
        }
    }
}

/// The controls of a window, in a header bar or inline toolbar.
pub(super) struct Toolbar {
    /// The bar with the controls.
    pub(super) bar: *mut c_void,
    view: Vec<*mut c_void>,
    edit: Vec<*mut c_void>,
    // The widget of every action, which is the menu button of the items of a menu.
    buttons: Vec<(&'static str, *mut c_void)>,
    // The toggled buttons by their action.
    toggles: Vec<(&'static str, *mut c_void)>,
    // The switches by their action.
    switches: Vec<(&'static str, *mut c_void)>,
}

impl Toolbar {
    /// Builds controls owned by either the window titlebar or the caller's content box, showing
    /// the view controls.
    pub(super) unsafe fn install(window: *mut c_void, integrated: bool) -> Self {
        // SAFETY: The window is live on the GTK thread. Containers own their children and
        // signal data points to static button descriptions for the lifetime of each widget.
        unsafe {
            if integrated {
                // GTK requires setting the titlebar before realizing the window.
                gtk_widget_hide(window);
                gtk_widget_unrealize(window);
            }
            let bar = if integrated {
                let bar = gtk_header_bar_new();
                gtk_header_bar_set_show_close_button(bar, 1);
                g_object_bind_property(
                    window,
                    c"title".as_ptr(),
                    bar,
                    c"title".as_ptr(),
                    G_BINDING_SYNC_CREATE,
                );
                bar
            } else {
                gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 6)
            };
            let mut toolbar = Self {
                bar,
                view: Vec::new(),
                edit: Vec::new(),
                buttons: Vec::new(),
                toggles: Vec::new(),
                switches: Vec::new(),
            };
            for (items, leading, editing) in [
                (VIEW_START, true, false),
                (VIEW_END, false, false),
                (EDIT_START, true, true),
                (EDIT_END, false, true),
            ] {
                for item in items {
                    let group = toolbar.add_item(item);
                    if !integrated {
                        gtk_widget_set_valign(group, GTK_ALIGN_CENTER);
                        gtk_widget_set_margin_top(group, 6);
                        gtk_widget_set_margin_bottom(group, 6);
                    }
                    match (integrated, leading) {
                        (true, true) => gtk_header_bar_pack_start(bar, group),
                        (true, false) => gtk_header_bar_pack_end(bar, group),
                        (false, true) => gtk_box_pack_start(bar, group, 0, 0, 6),
                        (false, false) => gtk_box_pack_end(bar, group, 0, 0, 6),
                    }
                    // Only the toolbar decides which controls show, also for showing all
                    // children of the window.
                    gtk_widget_show_all(group);
                    gtk_widget_set_no_show_all(group, 1);
                    if editing {
                        gtk_widget_hide(group);
                        toolbar.edit.push(group);
                    } else {
                        toolbar.view.push(group);
                    }
                }
            }
            if integrated {
                gtk_window_set_titlebar(window, bar);
                gtk_widget_show_all(window);
            } else {
                // Match the Windows header's 48 logical pixels, allowing larger GTK themes.
                gtk_widget_set_size_request(bar, -1, 48);
                gtk_style_context_add_class(
                    gtk_widget_get_style_context(bar),
                    c"primary-toolbar".as_ptr(),
                );
            }
            toolbar
        }
    }

    // Creates the widget of an item.
    unsafe fn add_item(&mut self, item: &'static Item) -> *mut c_void {
        // SAFETY: Containers own their children, the handlers take static descriptions.
        unsafe {
            let group = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
            gtk_style_context_add_class(gtk_widget_get_style_context(group), c"linked".as_ptr());
            match item {
                Item::Group(buttons) => {
                    for button in *buttons {
                        let widget = new_button(button);
                        gtk_button_set_focus_on_click(widget, 0);
                        connect(widget, c"clicked", clicked as *const c_void, button);
                        gtk_container_add(group, widget);
                        self.buttons.push((button.action, widget));
                    }
                }
                Item::Toggled(buttons) => {
                    let mut first = std::ptr::null_mut();
                    for button in *buttons {
                        let widget = gtk_radio_button_new_from_widget(first);
                        gtk_toggle_button_set_mode(widget, 0);
                        gtk_button_set_image(
                            widget,
                            gtk_image_new_from_icon_name(
                                button.icon.as_ptr(),
                                GTK_ICON_SIZE_BUTTON,
                            ),
                        );
                        gtk_widget_set_tooltip_text(widget, button.label.as_ptr());
                        gtk_button_set_focus_on_click(widget, 0);
                        connect(widget, c"toggled", toggled as *const c_void, button);
                        gtk_container_add(group, widget);
                        self.toggles.push((button.action, widget));
                        if first.is_null() {
                            first = widget;
                        }
                    }
                }
                Item::Switch(button) => {
                    let widget = gtk_toggle_button_new();
                    gtk_button_set_image(
                        widget,
                        gtk_image_new_from_icon_name(button.icon.as_ptr(), GTK_ICON_SIZE_BUTTON),
                    );
                    gtk_widget_set_tooltip_text(widget, button.label.as_ptr());
                    gtk_button_set_focus_on_click(widget, 0);
                    connect(widget, c"toggled", switched as *const c_void, button);
                    gtk_container_add(group, widget);
                    self.switches.push((button.action, widget));
                }
                Item::Menu(label, sections) => {
                    let widget = gtk_menu_button_new();
                    gtk_button_set_image(widget, gtk_label_new(label.as_ptr()));
                    gtk_button_set_focus_on_click(widget, 0);
                    let menu = gtk_menu_new();
                    for (index, section) in sections.iter().enumerate() {
                        if index > 0 {
                            gtk_menu_shell_append(menu, gtk_separator_menu_item_new());
                        }
                        for button in *section {
                            let entry = gtk_menu_item_new_with_label(button.label.as_ptr());
                            connect(entry, c"activate", clicked as *const c_void, button);
                            gtk_menu_shell_append(menu, entry);
                            self.buttons.push((button.action, widget));
                        }
                    }
                    gtk_widget_show_all(menu);
                    gtk_menu_button_set_popup(widget, menu);
                    gtk_container_add(group, widget);
                }
            }
            group
        }
    }

    /// Shows the edit controls, or the view controls.
    pub(super) fn set_editing(&self, editing: bool) {
        for (widgets, visible) in [(&self.view, !editing), (&self.edit, editing)] {
            for widget in widgets {
                // SAFETY: The bar owns its controls while the window is open.
                unsafe { gtk_widget_set_visible(*widget, i32::from(visible)) };
            }
        }
    }

    /// Enables the buttons that start editing, only still bitmaps can be edited.
    pub(super) fn set_editable(&self, editable: bool) {
        for (_, widget) in self
            .buttons
            .iter()
            .filter(|(action, _)| *action == "edit.toggle")
        {
            // SAFETY: The bar owns its controls while the window is open.
            unsafe { gtk_widget_set_sensitive(*widget, i32::from(editable)) };
        }
    }

    /// Shows the toggled button of an action as on.
    pub(super) fn set_toggled(&self, action: &str) {
        SETTING.set(true);
        for (_, widget) in self.toggles.iter().filter(|(name, _)| *name == action) {
            // SAFETY: The bar owns its controls while the window is open.
            unsafe { gtk_toggle_button_set_active(*widget, 1) };
        }
        SETTING.set(false);
    }

    /// Shows the switch of an action as on or off, without running the action.
    pub(super) fn set_switch(&self, action: &str, on: bool) {
        SETTING.set(true);
        for (_, widget) in self.switches.iter().filter(|(name, _)| *name == action) {
            // SAFETY: The bar owns its controls while the window is open.
            unsafe { gtk_toggle_button_set_active(*widget, i32::from(on)) };
        }
        SETTING.set(false);
    }

    /// Shows or hides the switch of an action, like the sidebar one of a file with one page.
    pub(super) fn set_switch_visible(&self, action: &str, visible: bool) {
        for (_, widget) in self.switches.iter().filter(|(name, _)| *name == action) {
            // SAFETY: The bar owns its controls while the window is open.
            unsafe { gtk_widget_set_visible(*widget, i32::from(visible)) };
        }
    }

    /// The control that runs an action, to show a popover at.
    pub(super) fn button(&self, action: &str) -> Option<*mut c_void> {
        self.buttons
            .iter()
            .find(|(name, _)| *name == action)
            .map(|(_, widget)| *widget)
    }
}

/// Shows the decoded image dimensions beneath the filename.
pub(super) unsafe fn show_size(bar: *mut c_void, media: &Media) {
    let (width, height) = media.size();
    let subtitle = CString::new(format!(
        "{} × {}",
        width.round() as i64,
        height.round() as i64
    ))
    .expect("image dimensions contain no NUL");
    // SAFETY: The caller supplies a live header bar; GTK copies the subtitle.
    unsafe { gtk_header_bar_set_subtitle(bar, subtitle.as_ptr()) };
}

#[cfg(test)]
mod tests {
    use super::desktop_uses_header_bar;

    #[test]
    fn desktop_conventions_choose_the_toolbar_location() {
        for desktop in [
            "GNOME",
            "ubuntu:GNOME",
            "GNOME-Classic:GNOME",
            "Budgie:GNOME",
            "Pantheon",
            "gnome",
        ] {
            assert!(desktop_uses_header_bar(desktop), "{desktop}");
        }
        for desktop in [
            "KDE",
            "MATE",
            "X-Cinnamon",
            "X-Cinnamon:GNOME",
            "XFCE",
            "LXQt",
            "Unity",
            "GNOME-Flashback:GNOME",
            "",
            "sway",
            "NotGNOME",
        ] {
            assert!(!desktop_uses_header_bar(desktop), "{desktop}");
        }
    }
}
