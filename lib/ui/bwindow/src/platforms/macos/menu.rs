/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use objc2::rc::Retained;
use objc2::runtime::AnyObject as Object;
use objc2::{class, msg_send, sel};

use super::headers::*;
use crate::EventLoopBuilder;

// MARK: NativeAccelerator
// An NSMenuItem accelerator: an NSEventModifierFlags mask and a key equivalent string
#[derive(Clone, Copy, PartialEq, Eq)]
struct NativeAccelerator {
    modifiers: u64,
    key: &'static str,
}

impl NativeAccelerator {
    // Shorthand for the Command + key accelerators that most default items use
    const fn command(key: &'static str) -> Self {
        Self {
            modifiers: NS_EVENT_MODIFIER_FLAG_COMMAND,
            key,
        }
    }
}

#[cfg(feature = "menu")]
impl From<crate::Accelerator> for NativeAccelerator {
    fn from(accelerator: crate::Accelerator) -> Self {
        use crate::{KeyCode, Modifiers};

        let mut modifiers = 0;
        if accelerator.modifiers.contains(Modifiers::COMMAND) {
            modifiers |= NS_EVENT_MODIFIER_FLAG_COMMAND;
        }
        if accelerator.modifiers.contains(Modifiers::CONTROL) {
            modifiers |= NS_EVENT_MODIFIER_FLAG_CONTROL;
        }
        if accelerator.modifiers.contains(Modifiers::OPTION) {
            modifiers |= NS_EVENT_MODIFIER_FLAG_OPTION;
        }
        if accelerator.modifiers.contains(Modifiers::SHIFT) {
            modifiers |= NS_EVENT_MODIFIER_FLAG_SHIFT;
        }
        Self {
            modifiers,
            key: match accelerator.key {
                KeyCode::KeyA => "a",
                KeyCode::KeyB => "b",
                KeyCode::KeyC => "c",
                KeyCode::KeyD => "d",
                KeyCode::KeyE => "e",
                KeyCode::KeyF => "f",
                KeyCode::KeyG => "g",
                KeyCode::KeyH => "h",
                KeyCode::KeyI => "i",
                KeyCode::KeyJ => "j",
                KeyCode::KeyK => "k",
                KeyCode::KeyL => "l",
                KeyCode::KeyM => "m",
                KeyCode::KeyN => "n",
                KeyCode::KeyO => "o",
                KeyCode::KeyP => "p",
                KeyCode::KeyQ => "q",
                KeyCode::KeyR => "r",
                KeyCode::KeyS => "s",
                KeyCode::KeyT => "t",
                KeyCode::KeyU => "u",
                KeyCode::KeyV => "v",
                KeyCode::KeyW => "w",
                KeyCode::KeyX => "x",
                KeyCode::KeyY => "y",
                KeyCode::KeyZ => "z",
                KeyCode::Digit0 => "0",
                KeyCode::Digit1 => "1",
                KeyCode::Digit2 => "2",
                KeyCode::Digit3 => "3",
                KeyCode::Digit4 => "4",
                KeyCode::Digit5 => "5",
                KeyCode::Digit6 => "6",
                KeyCode::Digit7 => "7",
                KeyCode::Digit8 => "8",
                KeyCode::Digit9 => "9",
                KeyCode::F1 => "\u{f704}",
                KeyCode::F2 => "\u{f705}",
                KeyCode::F3 => "\u{f706}",
                KeyCode::F4 => "\u{f707}",
                KeyCode::F5 => "\u{f708}",
                KeyCode::F6 => "\u{f709}",
                KeyCode::F7 => "\u{f70a}",
                KeyCode::F8 => "\u{f70b}",
                KeyCode::F9 => "\u{f70c}",
                KeyCode::F10 => "\u{f70d}",
                KeyCode::F11 => "\u{f70e}",
                KeyCode::F12 => "\u{f70f}",
                KeyCode::F13 => "\u{f710}",
                KeyCode::F14 => "\u{f711}",
                KeyCode::F15 => "\u{f712}",
                KeyCode::F16 => "\u{f713}",
                KeyCode::F17 => "\u{f714}",
                KeyCode::F18 => "\u{f715}",
                KeyCode::F19 => "\u{f716}",
                KeyCode::F20 => "\u{f717}",
                KeyCode::F21 => "\u{f718}",
                KeyCode::F22 => "\u{f719}",
                KeyCode::F23 => "\u{f71a}",
                KeyCode::F24 => "\u{f71b}",
                KeyCode::Backquote => "`",
                KeyCode::Backslash => "\\",
                KeyCode::BracketLeft => "[",
                KeyCode::BracketRight => "]",
                KeyCode::Comma => ",",
                KeyCode::Equal => "=",
                KeyCode::Minus => "-",
                KeyCode::Period => ".",
                KeyCode::Quote => "'",
                KeyCode::Semicolon => ";",
                KeyCode::Slash => "/",
                KeyCode::Space => " ",
                KeyCode::Tab => "\t",
                // The backspace key reports NSDeleteCharacter, NSBackspaceCharacter is never matched
                KeyCode::Backspace => "\u{7f}",
                KeyCode::Delete => "\u{f728}",
                KeyCode::Enter => "\r",
                KeyCode::Escape => "\u{1b}",
                KeyCode::ArrowUp => "\u{f700}",
                KeyCode::ArrowDown => "\u{f701}",
                KeyCode::ArrowLeft => "\u{f702}",
                KeyCode::ArrowRight => "\u{f703}",
                KeyCode::Home => "\u{f729}",
                KeyCode::End => "\u{f72b}",
                KeyCode::PageUp => "\u{f72c}",
                KeyCode::PageDown => "\u{f72d}",
            },
        }
    }
}

// MARK: Menu
#[derive(Clone, Copy)]
enum MenuRole {
    Application,
    Normal,
    Window,
    Help,
}

enum MenuItemAction {
    About,
    Hide,
    HideOthers,
    ShowAll,
    Terminate,
    Close,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    Minimize,
    Zoom,
    #[cfg(feature = "menu")]
    Custom(String),
}

struct NativeMenuItem {
    title: String,
    action: MenuItemAction,
    accelerator: Option<NativeAccelerator>,
}

impl NativeMenuItem {
    fn new(title: impl Into<String>, action: MenuItemAction) -> Self {
        Self {
            title: title.into(),
            action,
            accelerator: None,
        }
    }

    const fn accelerator(mut self, accelerator: NativeAccelerator) -> Self {
        self.accelerator = Some(accelerator);
        self
    }

    // Shorthand for the Command + key accelerators that most default items use
    const fn command(self, key: &'static str) -> Self {
        self.accelerator(NativeAccelerator::command(key))
    }

    // Creates the NSMenuItem carrying this item's title, accelerator and target action
    unsafe fn create_native(self, app_delegate: *mut Object) -> Retained<Object> {
        let native_item: Retained<Object> = unsafe { msg_send![class!(NSMenuItem), new] };
        let _: () = unsafe { msg_send![&native_item, setTitle:&*NSString::new(self.title)] };
        if let Some(accelerator) = self.accelerator {
            let _: () = unsafe {
                msg_send![&native_item, setKeyEquivalent:&*NSString::new(accelerator.key)]
            };
            let _: () = unsafe {
                msg_send![&native_item, setKeyEquivalentModifierMask:accelerator.modifiers]
            };
        }
        // Items left without a target travel the responder chain to whatever view has focus
        match self.action {
            MenuItemAction::About => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(openAboutDialog:)] };
                let _: () = unsafe { msg_send![&native_item, setTarget:app_delegate] };
            }
            MenuItemAction::Hide => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(hide:)] };
            }
            MenuItemAction::HideOthers => {
                let _: () =
                    unsafe { msg_send![&native_item, setAction:sel!(hideOtherApplications:)] };
            }
            MenuItemAction::ShowAll => {
                let _: () =
                    unsafe { msg_send![&native_item, setAction:sel!(unhideAllApplications:)] };
            }
            MenuItemAction::Terminate => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(terminate:)] };
            }
            MenuItemAction::Close => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(performClose:)] };
            }
            MenuItemAction::Undo => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(undo:)] };
            }
            MenuItemAction::Redo => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(redo:)] };
            }
            MenuItemAction::Cut => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(cut:)] };
            }
            MenuItemAction::Copy => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(copy:)] };
            }
            MenuItemAction::Paste => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(paste:)] };
            }
            MenuItemAction::Delete => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(delete:)] };
            }
            MenuItemAction::SelectAll => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(selectAll:)] };
            }
            MenuItemAction::Minimize => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(performMiniaturize:)] };
            }
            MenuItemAction::Zoom => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(performZoom:)] };
            }
            #[cfg(feature = "menu")]
            MenuItemAction::Custom(action) => {
                let _: () = unsafe { msg_send![&native_item, setAction:sel!(menuItemSelected:)] };
                let _: () = unsafe { msg_send![&native_item, setTarget:app_delegate] };
                let _: () = unsafe {
                    msg_send![&native_item, setRepresentedObject:&*NSString::new(action)]
                };
            }
        }
        native_item
    }
}

enum MenuEntry {
    Item(NativeMenuItem),
    Separator,
    /// Submenu that macOS populates with the available Services
    ServicesMenu(String),
}

impl MenuEntry {
    // Appends this entry to a native NSMenu
    unsafe fn add_to(
        self,
        native_menu: *mut Object,
        application: *mut Object,
        app_delegate: *mut Object,
    ) {
        let native_item: Retained<Object> = match self {
            MenuEntry::Separator => unsafe { msg_send![class!(NSMenuItem), separatorItem] },
            MenuEntry::ServicesMenu(title) => unsafe {
                let native_item: Retained<Object> = msg_send![class!(NSMenuItem), new];
                let _: () = msg_send![&native_item, setTitle:&*NSString::new(title)];
                let services_menu: Retained<Object> = msg_send![class!(NSMenu), new];
                let _: () = msg_send![&native_item, setSubmenu:services_menu.as_ptr()];
                let _: () = msg_send![application, setServicesMenu:services_menu.as_ptr()];
                native_item
            },
            MenuEntry::Item(item) => unsafe { item.create_native(app_delegate) },
        };
        let _: () = unsafe { msg_send![native_menu, addItem:native_item.as_ptr()] };
    }
}

struct Menu {
    title: String,
    role: MenuRole,
    entries: Vec<MenuEntry>,
}

impl Menu {
    fn new(title: impl Into<String>, role: MenuRole) -> Self {
        Self {
            title: title.into(),
            role,
            entries: Vec::new(),
        }
    }

    fn item(mut self, item: NativeMenuItem) -> Self {
        self.entries.push(MenuEntry::Item(item));
        self
    }

    fn separator(mut self) -> Self {
        self.entries.push(MenuEntry::Separator);
        self
    }

    fn services_menu(mut self, title: impl Into<String>) -> Self {
        self.entries.push(MenuEntry::ServicesMenu(title.into()));
        self
    }

    // Creates the menu bar entry that owns this menu's populated NSMenu
    unsafe fn create_native(
        self,
        application: *mut Object,
        app_delegate: *mut Object,
    ) -> Retained<Object> {
        let menu_item: Retained<Object> = unsafe { msg_send![class!(NSMenuItem), new] };
        // The application menu takes its title from the process name, so never set one
        if !matches!(self.role, MenuRole::Application) {
            let _: () = unsafe { msg_send![&menu_item, setTitle:&*NSString::new(self.title)] };
        }

        let native_menu: Retained<Object> = unsafe { msg_send![class!(NSMenu), new] };
        let _: () = unsafe { msg_send![&menu_item, setSubmenu:native_menu.as_ptr()] };
        match self.role {
            MenuRole::Window => {
                let _: () = unsafe { msg_send![application, setWindowsMenu:native_menu.as_ptr()] };
            }
            MenuRole::Help => {
                let _: () = unsafe { msg_send![application, setHelpMenu:native_menu.as_ptr()] };
            }
            MenuRole::Application | MenuRole::Normal => {}
        }

        for entry in self.entries {
            unsafe { entry.add_to(native_menu.as_ptr(), application, app_delegate) };
        }
        menu_item
    }
}

// The whole macOS menu bar: bwindow's defaults with the app's own menus folded in
struct MenuBar(Vec<Menu>);

impl MenuBar {
    // The complete menu bar every bwindow app gets before its own menus are merged in
    fn with_defaults(app_name: &str) -> Self {
        Self(vec![
            Menu::new(app_name, MenuRole::Application)
                .item(NativeMenuItem::new(
                    format!("About {app_name}"),
                    MenuItemAction::About,
                ))
                .separator()
                .services_menu("Services")
                .separator()
                .item(
                    NativeMenuItem::new(format!("Hide {app_name}"), MenuItemAction::Hide)
                        .command("h"),
                )
                .item(
                    NativeMenuItem::new("Hide Others", MenuItemAction::HideOthers).accelerator(
                        NativeAccelerator {
                            modifiers: NS_EVENT_MODIFIER_FLAG_COMMAND
                                | NS_EVENT_MODIFIER_FLAG_OPTION,
                            key: "h",
                        },
                    ),
                )
                .item(NativeMenuItem::new("Show All", MenuItemAction::ShowAll))
                .separator()
                .item(
                    NativeMenuItem::new(format!("Quit {app_name}"), MenuItemAction::Terminate)
                        .command("q"),
                ),
            Menu::new("File", MenuRole::Normal)
                .item(NativeMenuItem::new("Close Window", MenuItemAction::Close).command("w")),
            Menu::new("Edit", MenuRole::Normal)
                .item(NativeMenuItem::new("Undo", MenuItemAction::Undo).command("z"))
                .item(
                    NativeMenuItem::new("Redo", MenuItemAction::Redo).accelerator(
                        NativeAccelerator {
                            modifiers: NS_EVENT_MODIFIER_FLAG_COMMAND
                                | NS_EVENT_MODIFIER_FLAG_SHIFT,
                            key: "z",
                        },
                    ),
                )
                .separator()
                .item(NativeMenuItem::new("Cut", MenuItemAction::Cut).command("x"))
                .item(NativeMenuItem::new("Copy", MenuItemAction::Copy).command("c"))
                .item(NativeMenuItem::new("Paste", MenuItemAction::Paste).command("v"))
                .item(NativeMenuItem::new("Delete", MenuItemAction::Delete))
                .item(NativeMenuItem::new("Select All", MenuItemAction::SelectAll).command("a")),
            Menu::new("Window", MenuRole::Window)
                .item(NativeMenuItem::new("Minimize", MenuItemAction::Minimize).command("m"))
                .item(NativeMenuItem::new("Zoom", MenuItemAction::Zoom)),
            Menu::new("Help", MenuRole::Help),
        ])
    }

    // Installs this menu bar as the application's main menu
    unsafe fn create_native(self, application: *mut Object, app_delegate: *mut Object) {
        let menubar: Retained<Object> = unsafe { msg_send![class!(NSMenu), new] };
        let _: () = unsafe { msg_send![application, setMainMenu:menubar.as_ptr()] };
        for menu in self.0 {
            let menu_item = unsafe { menu.create_native(application, app_delegate) };
            let _: () = unsafe { msg_send![&menubar, addItem:menu_item.as_ptr()] };
        }
    }
}

#[cfg(feature = "menu")]
impl MenuBar {
    // Drops an accelerator everywhere it is already used, so app shortcuts beat the defaults
    fn clear_shortcut(&mut self, accelerator: NativeAccelerator) {
        for menu in &mut self.0 {
            for entry in &mut menu.entries {
                if let MenuEntry::Item(item) = entry
                    && item.accelerator == Some(accelerator)
                {
                    item.accelerator = None;
                }
            }
        }
    }

    // Folds the app's menus into the defaults. Menus and items matched by title are merged and
    // overridden in place, anything new is inserted ahead of the trailing system menus.
    fn merge(mut self, menu_bar: Option<crate::MenuBarBuilder>) -> Self {
        let Some(menu_bar) = menu_bar else {
            return self;
        };

        for custom_menu in menu_bar.menus {
            let menu_index = if let Some(index) = self
                .0
                .iter()
                .position(|menu| menu.title == custom_menu.title)
            {
                index
            } else {
                let index = self
                    .0
                    .iter()
                    .position(|menu| matches!(menu.role, MenuRole::Window | MenuRole::Help))
                    .unwrap_or(self.0.len());
                self.0
                    .insert(index, Menu::new(custom_menu.title, MenuRole::Normal));
                index
            };

            let mut insertion_index = 0;
            for entry in custom_menu.entries {
                match entry {
                    crate::MenuBuilderEntry::Separator => {
                        self.0[menu_index]
                            .entries
                            .insert(insertion_index, MenuEntry::Separator);
                        insertion_index += 1;
                    }
                    crate::MenuBuilderEntry::Item(custom_item) => {
                        let accelerator = custom_item.accelerator.map(NativeAccelerator::from);
                        if let Some(accelerator) = accelerator {
                            self.clear_shortcut(accelerator);
                        }
                        let item = NativeMenuItem {
                            title: custom_item.title,
                            action: MenuItemAction::Custom(custom_item.action),
                            accelerator,
                        };
                        let existing = self.0[menu_index].entries.iter().position(|entry| {
                            matches!(entry, MenuEntry::Item(existing) if existing.title == item.title)
                        });
                        if let Some(index) = existing {
                            self.0[menu_index].entries[index] = MenuEntry::Item(item);
                        } else {
                            self.0[menu_index]
                                .entries
                                .insert(insertion_index, MenuEntry::Item(item));
                            insertion_index += 1;
                        }
                    }
                }
            }
        }
        self
    }
}

/// Builds the default menu bar, folds in the app's custom menus and installs it
pub(super) unsafe fn create_menu_bar(
    application: *mut Object,
    app_delegate: *mut Object,
    builder: &mut EventLoopBuilder,
) {
    let app_name: NSString = unsafe { msg_send![application, valueForKey:ns_string!("name")] };
    let menu_bar = MenuBar::with_defaults(&app_name.to_string());
    #[cfg(feature = "menu")]
    let menu_bar = menu_bar.merge(builder.menu.take());
    #[cfg(not(feature = "menu"))]
    let _ = builder;
    unsafe { menu_bar.create_native(application, app_delegate) };
}
