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
    MenuBarBuilder, MenuBuilder, MenuItem, MessageDialog, MessageLevel, Modifiers, Theme, Window,
    WindowBuilder, WindowEvent, WindowId,
};
use imageview_core::{
    EXTENSIONS, FileVersion, Media, ZOOM_STEP, decode_media, is_supported, preferred_content_size,
};

use crate::platforms::MediaView;

mod platforms;
#[cfg(not(windows))]
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

// A window showing one image file.
struct ImageWindow {
    window: Window,
    view: MediaView,
    path: PathBuf,
    version: Option<FileVersion>,
    // Increments for every load into this window, so only the latest one is shown.
    generation: u64,
    // The open request that is still decoding the first media of the window.
    opening: Option<u64>,
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
        let (width, height) = media.as_ref().map_or(LOADING_SIZE, |media| {
            let (width, height) = media.size();
            preferred_content_size(width, height)
        });
        #[cfg(target_os = "macos")]
        let width = width.max(MINIMUM_TOOLBAR_WIDTH);
        let window = WindowBuilder::new()
            .title(title(&path, media.as_ref()))
            .size(LogicalSize::new(
                (width as f32).max(platforms::MIN_SIZE.0),
                (height as f32).max(platforms::MIN_SIZE.1),
            ))
            .min_size(LogicalSize::new(
                platforms::MIN_SIZE.0,
                platforms::MIN_SIZE.1,
            ))
            .center()
            .allow_file_drop(true)
            .build();
        let view = MediaView::new(&window, media, self.theme);
        self.active = Some(window.id());
        self.windows.push(ImageWindow {
            window,
            view,
            path,
            version,
            generation: 0,
            opening,
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
                    window.window.set_title(title(&load.path, Some(&media)));
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
                window.window.set_title(title(&load.path, Some(&media)));
                window.view.set_media(media, zoom_to_fit);
                window.path = load.path;
                window.version = load.version;
            }
            // A file that is being saved may not decode yet, the next change reloads it.
            (Target::Replace { .. }, Err(_)) => {}
            (Target::Window(id), Err(error)) => {
                let opening = self.opening.remove(&id).is_some();
                let index = self
                    .windows
                    .iter()
                    .position(|window| window.opening == Some(id));
                if opening || index.is_some() {
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
        let Some(window) = self.window(id) else {
            return;
        };
        // Supersede the first load too, so its completion cannot replace the dropped file.
        window.opening = None;
        // The window moves on right away, so the next browse continues from this file even
        // when it does not decode, and reloads watch it instead of the shown one.
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
            if window.opening.is_some() {
                continue;
            }
            let version = FileVersion::of(&window.path);
            if version.is_some() && version != window.version {
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
                self.windows.retain(|window| window.window.id() != id);
                self.forget(id);
            }
            _ => {}
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

// macOS shows the media size in the header subtitle.
#[cfg(target_os = "macos")]
fn title(path: &Path, _media: Option<&Media>) -> String {
    file_name(path)
}

// GTK desktops with a separate titlebar show dimensions there instead of in the toolbar.
#[cfg(not(any(target_os = "macos", windows)))]
fn title(path: &Path, media: Option<&Media>) -> String {
    if platforms::uses_header_bar() {
        return file_name(path);
    }
    media.map_or_else(
        || file_name(path),
        |media| imageview_core::window_title(&file_name(path), media.size()),
    )
}

// The file name and the media size once it is decoded.
#[cfg(windows)]
fn title(path: &Path, media: Option<&Media>) -> String {
    media.map_or_else(
        || file_name(path),
        |media| imageview_core::window_title(&file_name(path), media.size()),
    )
}

fn menu() -> MenuBarBuilder {
    let primary = |key| Accelerator::new(Modifiers::PRIMARY, key);
    let file = MenuBuilder::new("File")
        .item(MenuItem::new("Open...", "file.open").accelerator(primary(KeyCode::KeyO)));
    #[cfg(target_os = "macos")]
    let file = file
        .separator()
        .item(MenuItem::new("Print...", "file.print").accelerator(primary(KeyCode::KeyP)))
        .separator();
    MenuBarBuilder::new().menu(file).menu(
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

    event_loop.run(move |event| match event {
        Event::OpenFiles(paths) | Event::UserEvent(AppEvent::Open(paths)) => app.open(paths),
        Event::UserEvent(AppEvent::Launched) => {
            // Ask for a file when the application launched without one.
            if app.windows.is_empty() && app.pending_loads == 0 {
                app.show_open_dialog();
                if app.windows.is_empty() && app.pending_loads == 0 {
                    _ = app.proxy.exit();
                }
            }
        }
        Event::UserEvent(AppEvent::Tick) => app.reload_changed(),
        Event::UserEvent(AppEvent::Loaded(load, result)) => app.loaded(load, result),
        Event::UserEvent(AppEvent::SlowLoad(id)) => app.slow_load(id),
        Event::Action(action) => app.action(&action),
        Event::UserEvent(AppEvent::Action(action)) => app.action(action),
        Event::Window(id, event) => app.window_event(id, event),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_decode_with_their_pixel_sizes() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        for (name, size) in [
            ("dice.bmp", (800.0, 600.0)),
            ("dice.dib", (800.0, 600.0)),
            ("ball.gif", (100.0, 100.0)),
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
