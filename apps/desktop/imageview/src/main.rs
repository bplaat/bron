/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bwindow::{
    Accelerator, Event, EventLoopBuilder, EventLoopProxy, FileDialog, KeyCode, LogicalSize,
    MenuBarBuilder, MenuBuilder, MenuItem, MessageDialog, MessageLevel, Modifiers, Theme, Window,
    WindowBuilder, WindowEvent, WindowId,
};
use imageview_core::{
    EXTENSIONS, FileVersion, Media, ZOOM_STEP, decode_media, preferred_content_size, window_title,
};

use crate::platforms::MediaView;

mod platforms;

// How often open files are checked for changes.
const RELOAD_INTERVAL: Duration = Duration::from_secs(1);

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
}

/// Where decoded media is shown.
pub(crate) struct Load {
    path: PathBuf,
    version: Option<FileVersion>,
    target: Target,
}

enum Target {
    // A new window.
    Window,
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
}

struct App {
    proxy: Arc<EventLoopProxy<AppEvent>>,
    windows: Vec<ImageWindow>,
    active: Option<WindowId>,
    pending_loads: usize,
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

    fn open(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            self.load(path, Target::Window);
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
            (Target::Window, Ok(media)) => {
                platforms::note_recent_file(&load.path);
                let (width, height) = media.size();
                let (width, height) = preferred_content_size(width, height);
                let window = WindowBuilder::new()
                    .title(title(&load.path, &media))
                    .size(LogicalSize::new(width as f32, height as f32))
                    .min_size(LogicalSize::new(240.0, 180.0))
                    .center()
                    .build();
                let view = MediaView::new(&window, media, self.theme);
                self.active = Some(window.id());
                self.windows.push(ImageWindow {
                    window,
                    view,
                    path: load.path,
                    version: load.version,
                    generation: 0,
                });
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
                window.window.set_title(title(&load.path, &media));
                window.view.set_media(media, zoom_to_fit);
                window.path = load.path;
                window.version = load.version;
            }
            // A file that is being saved may not decode yet, the next change reloads it.
            (Target::Replace { .. }, Err(_)) => {}
            (Target::Window, Err(error)) => {
                MessageDialog::new()
                    .level(MessageLevel::Error)
                    .title(format!("Could not open {}", file_name(&load.path)))
                    .description(error)
                    .show();
            }
        }
        if self.windows.is_empty() && self.pending_loads == 0 {
            _ = self.proxy.exit();
        }
    }

    // Shows the image `offset` places from the active one in its folder.
    fn browse(&mut self, offset: isize) {
        let Some(window) = self.active_window() else {
            return;
        };
        let Some(path) = platforms::neighbour_path(&window.path, offset) else {
            return;
        };
        // The window moves on right away, so the next browse continues from this file even
        // when it does not decode, and reloads watch it instead of the shown one.
        window.path.clone_from(&path);
        window.version = FileVersion::of(&path);
        window.generation += 1;
        let target = Target::Replace {
            window: window.window.id(),
            generation: window.generation,
            zoom_to_fit: true,
        };
        self.load(path, target);
    }

    // Reloads files that changed on disk, keeping their zoom.
    fn reload_changed(&mut self) {
        let mut reloads = Vec::new();
        for window in &mut self.windows {
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
            WindowEvent::ThemeChanged(theme) => {
                self.theme = theme;
                if let Some(window) = self.window(id) {
                    window.view.theme_changed(theme);
                }
            }
            WindowEvent::CloseRequested(request) if !request.default_prevented() => {
                self.windows.retain(|window| window.window.id() != id);
                if self.active == Some(id) {
                    self.active = None;
                }
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

fn title(path: &Path, media: &Media) -> String {
    window_title(&file_name(path), media.size())
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
        Event::Action(action) => app.action(&action),
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
            ("dice.jfif", (800.0, 600.0)),
            ("dice.png", (800.0, 600.0)),
            ("dice.qoi", (800.0, 600.0)),
            ("ball.gif", (100.0, 100.0)),
            ("ball.apng", (100.0, 100.0)),
            ("tiger.svg", (600.0, 600.0)),
            ("tiger.tvg", (600.0, 600.0)),
        ] {
            let bytes = std::fs::read(examples.join(name)).expect("read example");
            let media = decode_media(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(media.size(), size, "{name}");
        }
    }
}
