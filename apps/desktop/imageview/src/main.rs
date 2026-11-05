/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bwindow::{
    Accelerator, Event, EventLoopBuilder, EventLoopProxy, FileDialog, KeyCode, LogicalSize,
    MenuBarBuilder, MenuBuilder, MenuItem, Modifiers, Theme, Window, WindowBuilder, WindowEvent,
    WindowId,
};
#[cfg(not(windows))]
use bwindow::{MessageDialog, MessageLevel};
use imageview_core::{
    EXTENSIONS, FileVersion, Media, ZOOM_STEP, decode_media, is_supported, preferred_content_size,
};

use crate::platforms::MediaView;

#[cfg(windows)]
mod alert;
mod edit;
mod marquee;
mod platforms;
mod save;
// The WinUI scroll viewer zooms and pans the media itself, Windows only crops and selects with it.
#[cfg_attr(windows, allow(dead_code))]
mod viewport;

// How often open files are checked for changes.
const RELOAD_INTERVAL: Duration = Duration::from_secs(1);
// How long a file may take to decode before its window opens with a loading indicator.
const SLOW_LOAD: Duration = Duration::from_millis(250);
// The size a window opens with before its media is decoded.
const LOADING_SIZE: (f64, f64) = (800.0, 600.0);
// The width new windows open with at least, so the whole macOS toolbar fits.
#[cfg(target_os = "macos")]
const MINIMUM_TOOLBAR_WIDTH: f64 = 600.0;

/// Events sent to the event loop from worker threads.
pub(crate) enum AppEvent {
    /// A window-attached WinUI dialog finished without blocking the event loop.
    #[cfg(windows)]
    AlertClosed(WindowId, i32),
    /// The application finished launching.
    Launched,
    /// Open files in new windows, like the macOS recent documents menu does.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Open(Vec<PathBuf>),
    /// Check open files for changes.
    Tick,
    /// A file finished decoding.
    Loaded(Load, Result<Media, String>),
    /// A file for a new window is still decoding after a while.
    SlowLoad(u64),
    /// Run a menu action from a native toolbar.
    #[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
    Action(&'static str),
    /// A control of the open edit tool of a window changed, with the values of all controls.
    ToolChanged(WindowId, usize, Vec<f32>),
    /// The preview of an edit tool of a window finished, with the smaller copy of the
    /// picture it was made from and its scale.
    Previewed(
        WindowId,
        u64,
        Media,
        Arc<imageview_core::edit::Picture>,
        f64,
    ),
    /// A save of a window finished in the background.
    Saved(WindowId, edit::Saved),
    /// An edit of a window finished in the background.
    Applied(
        WindowId,
        imageview_core::edit::Picture,
        imageview_core::edit::Step,
        bool,
    ),
}

/// Where decoded media is shown.
pub(crate) struct Load {
    path: PathBuf,
    version: Option<FileVersion>,
    target: Target,
}

enum Target {
    // A new window, for an open request.
    Window(u64),
    // An open window, replacing its media.
    Replace {
        window: WindowId,
        generation: u64,
        zoom_to_fit: bool,
    },
}

// A window showing one image file. The view is declared first so it drops while its window still
// exists, its native controls are torn down before the window that contains them.
struct ImageWindow {
    view: MediaView,
    window: Window,
    path: PathBuf,
    version: Option<FileVersion>,
    // Increments for every load into this window, so only the latest one is shown.
    generation: u64,
    // The open request that is still decoding the first media of the window.
    opening: Option<u64>,
    // While a replacement decodes, retain the file of the displayed pixels for failure.
    replacing: Option<(PathBuf, Option<FileVersion>)>,
    editing: Option<edit::Editing>,
    // Whether an edit is being applied or the image saved in the background.
    busy: bool,
    #[cfg(windows)]
    alerts: std::collections::VecDeque<alert::Alert>,
}

impl ImageWindow {
    #[cfg(windows)]
    fn is_modal(&self) -> bool {
        !self.alerts.is_empty()
    }

    #[cfg(not(windows))]
    const fn is_modal(&self) -> bool {
        false
    }
    const fn is_loading(&self) -> bool {
        self.opening.is_some() || self.replacing.is_some()
    }

    // Whether the media is being edited, which keeps it from reloading.
    const fn is_editing(&self) -> bool {
        self.editing.is_some()
    }
}

struct App {
    proxy: Arc<EventLoopProxy<AppEvent>>,
    windows: Vec<ImageWindow>,
    active: Option<WindowId>,
    pending_loads: usize,
    // The open requests that are decoding without a window yet.
    opening: HashMap<u64, PathBuf>,
    next_open: u64,
    theme: Theme,
}

impl App {
    fn position(&self, id: WindowId) -> Option<usize> {
        self.windows
            .iter()
            .position(|window| window.window.id() == id)
    }

    fn window(&mut self, id: WindowId) -> Option<&mut ImageWindow> {
        self.position(id).map(|index| &mut self.windows[index])
    }

    // Closes a window and forgets it.
    fn close_window(&mut self, id: WindowId) {
        self.windows.retain(|window| window.window.id() != id);
        self.forget(id);
    }

    // Stops treating a closed window as the active one.
    fn forget(&mut self, id: WindowId) {
        if self.active == Some(id) {
            self.active = None;
        }
    }

    fn active_window(&mut self) -> Option<&mut ImageWindow> {
        let id = self
            .active
            .or_else(|| self.windows.last().map(|window| window.window.id()))?;
        self.window(id)
    }

    // Decodes the file on a worker thread.
    fn load(&mut self, path: PathBuf, target: Target) {
        self.pending_loads += 1;
        let proxy = Arc::clone(&self.proxy);
        std::thread::spawn(move || {
            let version = FileVersion::of(&path);
            let result = std::fs::read(&path)
                .map_err(|error| error.to_string())
                .and_then(|bytes| decode_media(&bytes));
            _ = proxy.send_user_event(AppEvent::Loaded(
                Load {
                    path,
                    version,
                    target,
                },
                result,
            ));
        });
    }

    // Opens the files in new windows, which open with a loading indicator when decoding is
    // slow.
    fn open(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            let id = self.next_open;
            self.next_open += 1;
            self.opening.insert(id, path.clone());
            self.load(path, Target::Window(id));
            let proxy = Arc::clone(&self.proxy);
            std::thread::spawn(move || {
                std::thread::sleep(SLOW_LOAD);
                _ = proxy.send_user_event(AppEvent::SlowLoad(id));
            });
        }
    }

    // Opens a window for media, or a loading window without it.
    fn open_window(
        &mut self,
        path: PathBuf,
        version: Option<FileVersion>,
        media: Option<Media>,
        opening: Option<u64>,
    ) {
        let size = content_size(media.as_ref());
        let window = WindowBuilder::new()
            .title(title(&path, media.as_ref(), false))
            .size(size)
            .min_size(LogicalSize::new(
                platforms::MIN_SIZE.0,
                platforms::MIN_SIZE.1,
            ))
            .center()
            .allow_file_drop(true)
            .build();
        #[cfg(windows)]
        let mut window = window;
        let view = MediaView::new(&window, media, self.theme);
        #[cfg(windows)]
        // WinUI extends content into the title bar, changing the native client rectangle.
        {
            window.set_size(size);
            view.zoom_to_fit();
        }
        self.active = Some(window.id());
        self.windows.push(ImageWindow {
            #[cfg(windows)]
            alerts: std::collections::VecDeque::new(),
            window,
            view,
            path,
            version,
            generation: 0,
            opening,
            replacing: None,
            editing: None,
            busy: false,
        });
    }

    fn slow_load(&mut self, id: u64) {
        if let Some(path) = self.opening.remove(&id) {
            self.open_window(path, None, None, Some(id));
        }
    }

    fn show_open_dialog(&mut self) {
        let mut dialog = FileDialog::new()
            .title("Open")
            .add_filter("Images", EXTENSIONS);
        if let Some(index) = self.active.and_then(|id| self.position(id)) {
            dialog = dialog.parent(&self.windows[index].window);
        }
        if let Some(paths) = dialog.pick_files() {
            self.open(paths);
        }
    }

    fn loaded(&mut self, load: Load, result: Result<Media, String>) {
        self.pending_loads -= 1;
        match (load.target, result) {
            (Target::Window(id), Ok(media)) => {
                platforms::note_recent_file(&load.path);
                if self.opening.remove(&id).is_some() {
                    self.open_window(load.path, load.version, Some(media), None);
                } else if let Some(window) = self
                    .windows
                    .iter_mut()
                    .find(|window| window.opening == Some(id))
                {
                    window.opening = None;
                    window.version = load.version;
                    window
                        .window
                        .set_title(title(&load.path, Some(&media), false));
                    // The window opened with the loading size gets the size of its media.
                    window.window.set_size(content_size(Some(&media)));
                    #[cfg(target_os = "macos")]
                    window.view.center_window();
                    window.view.set_media(media, true);
                }
            }
            (
                Target::Replace {
                    window,
                    generation,
                    zoom_to_fit,
                },
                Ok(media),
            ) => {
                let Some(window) = self.window(window) else {
                    return;
                };
                if window.generation != generation {
                    return;
                }
                window.replacing = None;
                window
                    .window
                    .set_title(title(&load.path, Some(&media), false));
                window.view.set_media(media, zoom_to_fit);
                window.path = load.path;
                window.version = load.version;
            }
            // A file that is being saved may not decode yet, the next change reloads it.
            (
                Target::Replace {
                    window, generation, ..
                },
                Err(_),
            ) => {
                if let Some(window) = self.window(window)
                    && window.generation == generation
                    && let Some((path, version)) = window.replacing.take()
                {
                    // A failed reload waits for the next file change before trying again.
                    window.version = if path == load.path {
                        load.version
                    } else {
                        version
                    };
                    window.path = path;
                }
            }
            (Target::Window(id), Err(error)) => {
                let opening = self.opening.remove(&id).is_some();
                let index = self
                    .windows
                    .iter()
                    .position(|window| window.opening == Some(id));
                if opening || index.is_some() {
                    #[cfg(windows)]
                    self.open_error(load.path, load.version, index, error);
                    #[cfg(not(windows))]
                    {
                        if let Some(index) = index {
                            let window = self.windows.remove(index);
                            self.forget(window.window.id());
                        }
                        MessageDialog::new()
                            .level(MessageLevel::Error)
                            .title(format!("Could not open {}", file_name(&load.path)))
                            .description(error)
                            .show();
                    }
                }
            }
        }
        if self.windows.is_empty() && self.pending_loads == 0 {
            _ = self.proxy.exit();
        }
    }

    // Shows the image `offset` places from the active one in its folder.
    fn browse(&mut self, offset: isize) {
        let Some(window) = self
            .active_window()
            .filter(|window| window.opening.is_none())
        else {
            return;
        };
        let Some(path) = platforms::neighbour_path(&window.path, offset) else {
            return;
        };
        let id = window.window.id();
        self.show(id, path);
    }

    // Shows another image in a window, fitted to it.
    fn show(&mut self, id: WindowId, path: PathBuf) {
        if !self.finish_editing(id, edit::AfterSave::Show(path.clone())) {
            return;
        }
        let Some(window) = self.window(id) else {
            return;
        };
        // Supersede the first load too, so its completion cannot replace the dropped file.
        window.opening = None;
        // Browse continues from the requested file while it decodes. On failure, restore the
        // path of the displayed pixels so editing never pairs them with a different file.
        if window.replacing.is_none() {
            window.replacing = Some((window.path.clone(), window.version));
        }
        window.path.clone_from(&path);
        window.version = FileVersion::of(&path);
        window.generation += 1;
        let target = Target::Replace {
            window: id,
            generation: window.generation,
            zoom_to_fit: true,
        };
        self.load(path, target);
    }

    // Reloads files that changed on disk, keeping their zoom.
    fn reload_changed(&mut self) {
        let mut reloads = Vec::new();
        for window in &mut self.windows {
            if window.is_loading() || window.is_editing() || window.busy || window.is_modal() {
                continue;
            }
            let version = FileVersion::of(&window.path);
            if version.is_some() && version != window.version {
                window.replacing = Some((window.path.clone(), window.version));
                window.version = version;
                window.generation += 1;
                reloads.push((
                    window.path.clone(),
                    Target::Replace {
                        window: window.window.id(),
                        generation: window.generation,
                        zoom_to_fit: false,
                    },
                ));
            }
        }
        for (path, target) in reloads {
            self.load(path, target);
        }
    }

    fn action(&mut self, action: &str) {
        if self.active_window().is_some_and(|window| window.is_modal()) {
            return;
        }
        if self.edit_action(action) {
            return;
        }
        match action {
            "file.open" => self.show_open_dialog(),
            "view.previous" => self.browse(-1),
            "view.next" => self.browse(1),
            _ => {
                let Some(window) = self.active_window() else {
                    return;
                };
                match action {
                    #[cfg(target_os = "macos")]
                    "file.print" => window.view.print(),
                    "view.zoom_in" => window.view.zoom_by(ZOOM_STEP),
                    "view.zoom_out" => window.view.zoom_by(1.0 / ZOOM_STEP),
                    "view.actual_size" => window.view.actual_size(),
                    "view.zoom_to_fit" => window.view.zoom_to_fit(),
                    _ => {}
                }
            }
        }
    }

    fn window_event(&mut self, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::Focus => self.active = Some(id),
            // A dropped image replaces the one in the window it is dropped on.
            WindowEvent::DroppedFile(path) if is_supported(&path) => self.show(id, path),
            WindowEvent::ThemeChanged(theme) => {
                self.theme = theme;
                if let Some(window) = self.window(id) {
                    window.view.theme_changed(theme);
                }
            }
            WindowEvent::CloseRequested(request) if !request.default_prevented() => {
                if !self.finish_editing(id, edit::AfterSave::Close) {
                    request.prevent_default();
                    return;
                }
                self.close_window(id);
            }
            _ => {}
        }
    }

    // Handles an event of the event loop.
    fn handle(&mut self, event: Event<AppEvent>) {
        match event {
            #[cfg(windows)]
            Event::UserEvent(AppEvent::AlertClosed(id, result)) => self.alert_closed(id, result),
            Event::LoopExiting => {
                self.windows.clear();
                #[cfg(windows)]
                platforms::shutdown();
            }
            Event::OpenFiles(paths) | Event::UserEvent(AppEvent::Open(paths)) => self.open(paths),
            Event::UserEvent(AppEvent::Launched) => {
                // Ask for a file when the application launched without one.
                if self.windows.is_empty() && self.pending_loads == 0 {
                    self.show_open_dialog();
                    if self.windows.is_empty() && self.pending_loads == 0 {
                        _ = self.proxy.exit();
                    }
                }
            }
            Event::UserEvent(AppEvent::Tick) => self.reload_changed(),
            Event::UserEvent(AppEvent::Loaded(load, result)) => self.loaded(load, result),
            Event::UserEvent(AppEvent::SlowLoad(id)) => self.slow_load(id),
            Event::Action(action) => self.action(&action),
            Event::UserEvent(AppEvent::Action(action)) => self.action(action),
            Event::UserEvent(AppEvent::ToolChanged(id, index, values)) => {
                self.tool_changed(id, index, values)
            }
            Event::UserEvent(AppEvent::Previewed(id, generation, media, proxy, scale)) => {
                self.previewed(id, generation, media, proxy, scale);
            }
            Event::UserEvent(AppEvent::Applied(id, picture, step, amend)) => {
                self.applied(id, picture, step, amend);
            }
            Event::UserEvent(AppEvent::Saved(id, saved)) => self.saved(id, saved),
            Event::Window(id, event) => self.window_event(id, event),
        }
    }
}

// The content size a window opens with for media, or while the media loads.
fn content_size(media: Option<&Media>) -> LogicalSize {
    let (width, height) = media.map_or(LOADING_SIZE, |media| {
        let (width, height) = media.size();
        preferred_content_size(width, height)
    });
    #[cfg(target_os = "macos")]
    let width = width.max(MINIMUM_TOOLBAR_WIDTH);
    #[cfg(windows)]
    let height = height + f64::from(platforms::CONTENT_HEADER_HEIGHT);
    LogicalSize::new(
        (width as f32).max(platforms::MIN_SIZE.0),
        (height as f32).max(platforms::MIN_SIZE.1),
    )
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

// macOS shows the media size in the header subtitle, and unsaved changes with a dot in the close
// button.
#[cfg(target_os = "macos")]
fn title(path: &Path, _media: Option<&Media>, _edited: bool) -> String {
    file_name(path)
}

// The file name with a star in front of it for unsaved changes, like other Windows and Linux
// applications, and the media size once it is decoded. GTK desktops with a header bar show the
// size in the toolbar instead.
#[cfg(not(target_os = "macos"))]
fn title(path: &Path, media: Option<&Media>, edited: bool) -> String {
    let name = if edited {
        format!("*{}", file_name(path))
    } else {
        file_name(path)
    };
    #[cfg(not(windows))]
    if platforms::uses_header_bar() {
        return name;
    }
    media.map_or_else(
        || name.clone(),
        |media| imageview_core::window_title(&name, media.size()),
    )
}

fn menu() -> MenuBarBuilder {
    let primary = |key| Accelerator::new(Modifiers::PRIMARY, key);
    let file = MenuBuilder::new("File")
        .item(MenuItem::new("Open...", "file.open").accelerator(primary(KeyCode::KeyO)));
    let file = file
        .separator()
        .item(MenuItem::new("Save", "file.save").accelerator(primary(KeyCode::KeyS)))
        .item(
            MenuItem::new("Save As...", "file.save_as").accelerator(Accelerator::new(
                Modifiers::PRIMARY | Modifiers::SHIFT,
                KeyCode::KeyS,
            )),
        )
        .separator();
    #[cfg(target_os = "macos")]
    let file = file
        .item(MenuItem::new("Print...", "file.print").accelerator(primary(KeyCode::KeyP)))
        .separator();
    let menu_bar = MenuBarBuilder::new().menu(file);
    let menu_bar = menu_bar.menu(edit_menu()).menu(image_menu());
    menu_bar.menu(
        MenuBuilder::new("View")
            .item(MenuItem::new("Zoom In", "view.zoom_in").accelerator(primary(KeyCode::Equal)))
            .item(MenuItem::new("Zoom Out", "view.zoom_out").accelerator(primary(KeyCode::Minus)))
            .separator()
            .item(
                MenuItem::new("Actual Size", "view.actual_size")
                    .accelerator(primary(KeyCode::Digit0)),
            )
            .item(
                MenuItem::new("Zoom to Fit", "view.zoom_to_fit")
                    .accelerator(primary(KeyCode::Digit9)),
            )
            .separator()
            .item(
                MenuItem::new("Previous Image", "view.previous")
                    .accelerator(Accelerator::new(Modifiers::NONE, KeyCode::ArrowLeft)),
            )
            .item(
                MenuItem::new("Next Image", "view.next")
                    .accelerator(Accelerator::new(Modifiers::NONE, KeyCode::ArrowRight)),
            ),
    )
}

// The selection items follow the default ones, Undo and Redo reach the image view through the
// responder chain.
#[cfg(target_os = "macos")]
fn edit_menu() -> MenuBuilder {
    MenuBuilder::new("Edit")
        .item(
            MenuItem::new("Deselect", "select.none")
                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyD)),
        )
        .item(
            MenuItem::new("Invert Selection", "select.invert").accelerator(Accelerator::new(
                Modifiers::PRIMARY | Modifiers::SHIFT,
                KeyCode::KeyI,
            )),
        )
}

// These take the place of the default items of the same names, which only reach text fields.
#[cfg(not(target_os = "macos"))]
fn edit_menu() -> MenuBuilder {
    let primary = |key| Accelerator::new(Modifiers::PRIMARY, key);
    MenuBuilder::new("Edit")
        .item(MenuItem::new("Undo", "edit.undo").accelerator(primary(KeyCode::KeyZ)))
        .item(
            MenuItem::new("Redo", "edit.redo").accelerator(Accelerator::new(
                Modifiers::PRIMARY | Modifiers::SHIFT,
                KeyCode::KeyZ,
            )),
        )
        .item(MenuItem::new("Copy", "edit.copy").accelerator(primary(KeyCode::KeyC)))
        .item(MenuItem::new("Select All", "select.all").accelerator(primary(KeyCode::KeyA)))
}

fn image_menu() -> MenuBuilder {
    let primary = |key| Accelerator::new(Modifiers::PRIMARY, key);
    let menu = MenuBuilder::new("Image")
        .item(MenuItem::new("Edit Image", "edit.toggle").accelerator(primary(KeyCode::KeyE)))
        .separator()
        .item(MenuItem::new("Whole Image", "select.whole"))
        .item(MenuItem::new("Rectangle Selection", "select.rectangle"))
        .item(MenuItem::new("Ellipse Selection", "select.ellipse"));
    // Elsewhere the Edit menu has no room for these, so they stay with the other selection items.
    #[cfg(not(target_os = "macos"))]
    let menu = menu
        .item(
            MenuItem::new("Deselect", "select.none")
                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyD)),
        )
        .item(
            MenuItem::new("Invert Selection", "select.invert").accelerator(Accelerator::new(
                Modifiers::PRIMARY | Modifiers::SHIFT,
                KeyCode::KeyI,
            )),
        );
    menu.separator()
        .item(MenuItem::new("Rotate Left", "image.rotate_left").accelerator(primary(KeyCode::KeyL)))
        .item(
            MenuItem::new("Rotate Right", "image.rotate_right").accelerator(primary(KeyCode::KeyR)),
        )
        .item(MenuItem::new("Flip Horizontal", "image.flip_horizontal"))
        .item(MenuItem::new("Flip Vertical", "image.flip_vertical"))
        .separator()
        .item(MenuItem::new("Crop...", "image.crop").accelerator(primary(KeyCode::KeyK)))
        .item(MenuItem::new("Resize...", "image.resize"))
        .separator()
        .item(
            MenuItem::new("Adjust...", "filter.adjust").accelerator(Accelerator::new(
                Modifiers::PRIMARY | Modifiers::OPTION,
                KeyCode::KeyC,
            )),
        )
        .item(
            MenuItem::new("Auto Enhance", "filter.auto_enhance").accelerator(Accelerator::new(
                Modifiers::PRIMARY | Modifiers::SHIFT,
                KeyCode::KeyE,
            )),
        )
        .separator()
        .item(MenuItem::new("Grayscale", "filter.grayscale"))
        .item(MenuItem::new("Sepia", "filter.sepia"))
        .item(MenuItem::new("Invert", "filter.invert"))
        .item(MenuItem::new("Threshold...", "filter.threshold"))
        .item(MenuItem::new("Solarize...", "filter.solarize"))
        .separator()
        .item(MenuItem::new("Gaussian Blur...", "filter.gaussian_blur"))
        .item(MenuItem::new("Box Blur...", "filter.box_blur"))
        .item(MenuItem::new("Motion Blur...", "filter.motion_blur"))
        .item(MenuItem::new("Radial Blur...", "filter.radial_blur"))
        .separator()
        .item(MenuItem::new("Sharpen...", "filter.sharpen"))
        .item(MenuItem::new("Reduce Noise...", "filter.reduce_noise"))
        .item(MenuItem::new("Pixelate...", "filter.pixelate"))
}

fn main() {
    // The freedesktop thumbnailer runs this binary with the thumbnailer arguments.
    #[cfg(not(any(target_os = "macos", windows)))]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--thumbnail")
    {
        std::process::exit(platforms::thumbnail(std::env::args_os().skip(2).collect()));
    }

    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "ImageView")
        // Windows does not forward the files of later launches, so every launch opens its own.
        .single_instance(!cfg!(windows))
        .menu(menu())
        .build();
    let proxy = Arc::new(event_loop.create_proxy());
    platforms::init(event_loop.create_proxy());

    let ticker = Arc::clone(&proxy);
    std::thread::spawn(move || {
        while ticker.send_user_event(AppEvent::Tick).is_ok() {
            std::thread::sleep(RELOAD_INTERVAL);
        }
    });

    let mut app = App {
        proxy: Arc::clone(&proxy),
        windows: Vec::new(),
        active: None,
        pending_loads: 0,
        opening: HashMap::new(),
        next_open: 0,
        theme: event_loop.theme(),
    };
    let files: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter(|argument| !argument.as_encoded_bytes().starts_with(b"-"))
        .map(PathBuf::from)
        .collect();
    if files.is_empty() {
        _ = proxy.send_user_event(AppEvent::Launched);
    } else if cfg!(any(target_os = "macos", windows)) {
        // GTK passes the command line files as an open event, the other platforms leave
        // them to us.
        app.open(files);
    }

    event_loop.run(move |event| {
        app.handle(event);
        // The menu follows the mode of the active window.
        app.update_menu();
    });
}

// MARK: Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn window_size_reserves_the_header_above_the_image() {
        let bytes =
            std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/dice.png")).unwrap();
        let media = decode_media(&bytes).unwrap();
        let size = content_size(Some(&media));
        assert_eq!(size.width, 800.0);
        assert_eq!(size.height, 600.0 + platforms::CONTENT_HEADER_HEIGHT);
        assert_eq!(
            content_size(None).height,
            LOADING_SIZE.1 as f32 + platforms::CONTENT_HEADER_HEIGHT
        );
    }

    #[test]
    fn examples_decode_with_their_pixel_sizes() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        for (name, size) in [
            ("dice.bmp", (800.0, 600.0)),
            ("dice.dib", (800.0, 600.0)),
            ("ball.gif", (100.0, 100.0)),
            ("ball.webp", (100.0, 100.0)),
            ("dice.j2k", (800.0, 600.0)),
            ("dice.jfif", (800.0, 600.0)),
            ("dice.png", (800.0, 600.0)),
            ("ball.apng", (100.0, 100.0)),
            ("dice.qoi", (800.0, 600.0)),
            ("dice.tga", (800.0, 600.0)),
            ("tiger.svg", (600.0, 600.0)),
            ("tiger.tvg", (600.0, 600.0)),
        ] {
            let bytes = std::fs::read(examples.join(name)).expect("read example");
            let media = decode_media(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(media.size(), size, "{name}");
        }
    }
}
