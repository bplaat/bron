/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native header bar or inline toolbar controls, running the same actions as the menu.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_void};

use bwindow::EventLoopProxy;
use imageview_core::Media;

use super::headers::*;
use crate::AppEvent;

thread_local! {
    static PROXY: RefCell<Option<EventLoopProxy<AppEvent>>> = const { RefCell::new(None) };
}

pub(crate) fn init(proxy: EventLoopProxy<AppEvent>) {
    PROXY.with_borrow_mut(|stored| *stored = Some(proxy));
}

struct Button {
    label: &'static CStr,
    icon: &'static CStr,
    action: &'static str,
}

const NAVIGATION: &[Button] = &[
    Button {
        label: c"Previous Image",
        icon: c"go-previous-symbolic",
        action: "view.previous",
    },
    Button {
        label: c"Next Image",
        icon: c"go-next-symbolic",
        action: "view.next",
    },
];
const ZOOM: &[Button] = &[
    Button {
        label: c"Zoom Out",
        icon: c"zoom-out-symbolic",
        action: "view.zoom_out",
    },
    Button {
        label: c"Actual Size",
        icon: c"zoom-original-symbolic",
        action: "view.actual_size",
    },
    Button {
        label: c"Zoom In",
        icon: c"zoom-in-symbolic",
        action: "view.zoom_in",
    },
];

extern "C" fn clicked(_: *mut c_void, button: &Button) {
    PROXY.with_borrow(|proxy| {
        if let Some(proxy) = proxy {
            _ = proxy.send_user_event(AppEvent::Action(button.action));
        }
    });
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

/// Builds controls owned by either the window titlebar or the caller's content box.
pub(super) unsafe fn install(window: *mut c_void, integrated: bool) -> *mut c_void {
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
        for (buttons, leading) in [(NAVIGATION, true), (ZOOM, false)] {
            let group = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 0);
            gtk_style_context_add_class(gtk_widget_get_style_context(group), c"linked".as_ptr());
            if !integrated {
                gtk_widget_set_valign(group, GTK_ALIGN_CENTER);
                gtk_widget_set_margin_top(group, 6);
                gtk_widget_set_margin_bottom(group, 6);
            }
            for button in buttons {
                let widget =
                    gtk_button_new_from_icon_name(button.icon.as_ptr(), GTK_ICON_SIZE_BUTTON);
                gtk_widget_set_tooltip_text(widget, button.label.as_ptr());
                gtk_button_set_focus_on_click(widget, 0);
                g_signal_connect_data(
                    widget,
                    c"clicked".as_ptr(),
                    clicked as *const c_void,
                    std::ptr::from_ref(button).cast(),
                    std::ptr::null(),
                    0,
                );
                gtk_container_add(group, widget);
            }
            if integrated {
                if leading {
                    gtk_header_bar_pack_start(bar, group);
                } else {
                    gtk_header_bar_pack_end(bar, group);
                }
            } else if leading {
                gtk_box_pack_start(bar, group, 0, 0, 6);
            } else {
                gtk_box_pack_end(bar, group, 0, 0, 6);
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
        bar
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
