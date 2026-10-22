/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::{KeyCode, Modifiers};

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
