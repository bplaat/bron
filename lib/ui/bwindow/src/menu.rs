/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "menu")]
use std::cell::RefCell;
#[cfg(feature = "menu")]
use std::collections::HashMap;

use crate::{KeyCode, Modifiers};

// The changed state of the menu items of an action.
#[cfg(feature = "menu")]
#[derive(Default)]
struct ItemState {
    disabled: bool,
    title: Option<String>,
}

#[cfg(feature = "menu")]
thread_local! {
    static ITEM_STATES: RefCell<HashMap<String, ItemState>> = RefCell::new(HashMap::new());
}

// Changes the state of an action, only allocating its key the first time.
#[cfg(feature = "menu")]
fn update_item_state<R>(action: &str, update: impl FnOnce(&mut ItemState) -> R) -> R {
    ITEM_STATES.with_borrow_mut(|states| {
        if let Some(state) = states.get_mut(action) {
            return update(state);
        }
        update(states.entry(action.to_owned()).or_default())
    })
}

/// Enables or disables the menu items and keyboard accelerators of an action. Disabled items
/// are grayed out on macOS and GTK, and their action events are not sent.
#[cfg(feature = "menu")]
pub fn set_menu_item_enabled(action: &str, enabled: bool) {
    let changed = update_item_state(action, |state| {
        std::mem::replace(&mut state.disabled, !enabled) == enabled
    });
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    if changed {
        crate::platforms::set_action_enabled(action, enabled);
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    let _ = changed;
}

/// Changes the title of the menu items of an action, which macOS shows the next time the menu
/// opens.
#[cfg(feature = "menu")]
pub fn set_menu_item_title(action: &str, title: impl Into<String>) {
    let title = title.into();
    update_item_state(action, |state| state.title = Some(title));
}

/// Whether the menu items of an action are enabled, which they are until disabled.
#[cfg(feature = "menu")]
pub fn is_menu_item_enabled(action: &str) -> bool {
    ITEM_STATES.with_borrow(|states| states.get(action).is_none_or(|state| !state.disabled))
}

// The changed title of the menu items of an action.
#[cfg(all(feature = "menu", target_os = "macos"))]
pub(crate) fn menu_item_title(action: &str) -> Option<String> {
    ITEM_STATES.with_borrow(|states| states.get(action).and_then(|state| state.title.clone()))
}

// The event of a chosen menu item or accelerator, none while its action is disabled.
#[cfg(feature = "menu")]
pub(crate) fn action_event<T>(action: String) -> Option<crate::Event<T>> {
    is_menu_item_enabled(&action).then_some(crate::Event::Action(action))
}

/// Application keyboard accelerator
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Accelerator {
    pub(crate) modifiers: Modifiers,
    pub(crate) key: KeyCode,
}

impl Accelerator {
    /// Create an accelerator from modifiers and a physical key code
    pub const fn new(modifiers: Modifiers, key: KeyCode) -> Self {
        Self { modifiers, key }
    }
}

/// Application command, optionally displayed in a native or desktop global menu.
pub struct MenuItem {
    pub(crate) title: String,
    pub(crate) action: String,
    pub(crate) accelerator: Option<Accelerator>,
}

impl MenuItem {
    /// Create a menu item with a title and stable action identifier
    pub fn new(title: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            action: action.into(),
            accelerator: None,
        }
    }

    /// Set the item's keyboard accelerator
    pub const fn accelerator(mut self, accelerator: Accelerator) -> Self {
        self.accelerator = Some(accelerator);
        self
    }
}

pub(crate) enum MenuBuilderEntry {
    Item(MenuItem),
    Separator,
}

/// Builder for a group of application commands.
pub struct MenuBuilder {
    pub(crate) title: String,
    pub(crate) entries: Vec<MenuBuilderEntry>,
}

impl MenuBuilder {
    /// Create an empty named menu
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            entries: Vec::new(),
        }
    }

    /// Add a menu item
    pub fn item(mut self, item: MenuItem) -> Self {
        self.entries.push(MenuBuilderEntry::Item(item));
        self
    }

    /// Add a separator
    pub fn separator(mut self) -> Self {
        self.entries.push(MenuBuilderEntry::Separator);
        self
    }
}

/// Application commands and shortcuts, merged into the native menu bar on macOS.
/// GTK merges standard commands, exports the menu for global bars and hides it inside the
/// window. Windows registers shortcuts without displaying a menu bar.
#[derive(Default)]
pub struct MenuBarBuilder {
    pub(crate) menus: Vec<MenuBuilder>,
}

impl MenuBarBuilder {
    /// Create an empty menu bar builder
    pub const fn new() -> Self {
        Self { menus: Vec::new() }
    }

    /// Add or extend a menu
    pub fn menu(mut self, menu: MenuBuilder) -> Self {
        self.menus.push(menu);
        self
    }
    // Windows registers the accelerators of all items, GTK tests its merged menus.
    #[cfg(any(
        windows,
        all(test, not(any(target_vendor = "apple", target_os = "android")))
    ))]
    pub(crate) fn items(&self) -> impl Iterator<Item = &MenuItem> {
        self.menus
            .iter()
            .flat_map(|menu| &menu.entries)
            .filter_map(|entry| match entry {
                MenuBuilderEntry::Item(item) => Some(item),
                MenuBuilderEntry::Separator => None,
            })
    }
}

#[cfg(all(test, feature = "menu"))]
mod tests {
    use super::*;
    use crate::Event;

    #[test]
    fn disabled_actions_send_no_events() {
        assert!(is_menu_item_enabled("test.open"));
        assert!(matches!(
            action_event::<()>("test.open".into()),
            Some(Event::Action(action)) if action == "test.open"
        ));
        set_menu_item_enabled("test.open", false);
        assert!(!is_menu_item_enabled("test.open"));
        assert!(action_event::<()>("test.open".into()).is_none());
        set_menu_item_enabled("test.open", true);
        assert!(action_event::<()>("test.open".into()).is_some());
    }
}
