/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, CString, c_int, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

use super::event_loop::{self, DISPLAY, Queue, android_post, post_and_wait};
use super::headers::*;
use super::jni::{self, JniEnv};
use super::window;

// The framework android.app.NativeActivity hosts the single window, its callbacks run on
// the Android UI thread. The process runs one Rust main and exits with the activity.
static ACTIVITY: AtomicPtr<ANativeActivity> = AtomicPtr::new(null_mut());
static ACTIVITY_OBJECT: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static UI_QUEUE: Queue = Queue::new();

/// The JNI global reference to the running `android.app.NativeActivity`.
pub fn android_activity() -> jobject {
    ACTIVITY_OBJECT.load(Ordering::Acquire)
}

/// Run `task` on the Android UI thread, callable from every thread.
pub fn android_run_on_ui_thread(task: impl FnOnce() + Send + 'static) {
    UI_QUEUE.post(Box::new(task));
}

/// Finish the activity, whose onDestroy ends the process, callable from every thread.
pub(super) fn finish() {
    let activity = ACTIVITY.load(Ordering::Acquire);
    if !activity.is_null() {
        unsafe { ANativeActivity_finish(activity) };
    }
}

// Exported to the framework NativeActivity, which loads the library named by its
// android.app.lib_name meta-data and calls this from Activity.onCreate.
#[unsafe(no_mangle)]
unsafe extern "C" fn ANativeActivity_onCreate(
    activity: *mut ANativeActivity,
    _: *mut c_void,
    _: usize,
) {
    unsafe {
        // The Rust main function runs once per process, so another instance only finishes.
        if !ACTIVITY.load(Ordering::Acquire).is_null() {
            ANativeActivity_finish(activity);
            return;
        }
        let callbacks = &mut *(*activity).callbacks;
        callbacks.onDestroy = Some(on_destroy);
        callbacks.onWindowFocusChanged = Some(on_window_focus_changed);
        callbacks.onNativeWindowCreated = Some(on_native_window_created);
        callbacks.onNativeWindowResized = Some(on_native_window_resized);
        callbacks.onNativeWindowRedrawNeeded = Some(on_native_window_redraw_needed);
        callbacks.onNativeWindowDestroyed = Some(on_native_window_destroyed);
        callbacks.onInputQueueCreated = Some(on_input_queue_created);
        callbacks.onInputQueueDestroyed = Some(on_input_queue_destroyed);
        callbacks.onContentRectChanged = Some(on_content_rect_changed);
        callbacks.onConfigurationChanged = Some(on_configuration_changed);

        let env = JniEnv::from_raw((*activity).env);
        jni::init((*activity).vm, env, (*activity).clazz);
        ACTIVITY_OBJECT.store(env.new_global_ref((*activity).clazz), Ordering::Release);
        ACTIVITY.store(activity, Ordering::Release);
        UI_QUEUE.attach();

        // NativeActivity defaults to a 16-bit RGB_565 window.
        ANativeActivity_setWindowFormat(activity, WINDOW_FORMAT_RGBA_8888);
        // Follow the system theme until the app calls set_theme, also after a crash.
        set_night_mode(env, NIGHT_MODE_AUTO);
        draw_behind_system_bars(env, (*activity).sdkVersion);
        update_configuration(activity);
        event_loop::start(binary_name());
    }
}

// MARK: Callbacks
unsafe extern "C" fn on_destroy(activity: *mut ANativeActivity) {
    if activity != ACTIVITY.load(Ordering::Acquire) {
        return;
    }
    // The system persists the application night mode, don't leak set_theme into the next launch.
    set_night_mode(
        unsafe { JniEnv::from_raw((*activity).env) },
        NIGHT_MODE_AUTO,
    );
    // The Rust main function runs once per process, start a fresh process next time.
    std::process::exit(0);
}

unsafe extern "C" fn on_window_focus_changed(_: *mut ANativeActivity, focused: c_int) {
    android_post(move || {
        window::send(if focused != 0 {
            crate::WindowEvent::Focus
        } else {
            crate::WindowEvent::Blur
        })
    });
}

// Native windows and input queues are shared with the Rust main thread, which owns them
// from their creation until the matching destroy callback returns.
#[derive(Clone, Copy)]
struct Shared<T>(*mut T);

// SAFETY: The framework keeps the pointee alive until its destroy callback returns.
unsafe impl<T> Send for Shared<T> {}

unsafe extern "C" fn on_native_window_created(_: *mut ANativeActivity, native: *mut ANativeWindow) {
    let native = Shared(native);
    android_post(move || {
        let native = native;
        event_loop::set_window(native.0);
        window::resized();
    });
}

unsafe extern "C" fn on_native_window_resized(_: *mut ANativeActivity, native: *mut ANativeWindow) {
    let native = Shared(native);
    android_post(move || {
        let native = native;
        event_loop::update_window_size(native.0);
        window::resized();
    });
}

unsafe extern "C" fn on_native_window_redraw_needed(
    _: *mut ANativeActivity,
    _: *mut ANativeWindow,
) {
    android_post(window::redraw);
}

unsafe extern "C" fn on_native_window_destroyed(_: *mut ANativeActivity, _: *mut ANativeWindow) {
    post_and_wait(|| event_loop::set_window(null_mut()));
}

unsafe extern "C" fn on_input_queue_created(_: *mut ANativeActivity, queue: *mut AInputQueue) {
    let queue = Shared(queue);
    android_post(move || {
        let queue = queue;
        event_loop::attach_input(queue.0);
    });
}

unsafe extern "C" fn on_input_queue_destroyed(_: *mut ANativeActivity, queue: *mut AInputQueue) {
    let queue = Shared(queue);
    post_and_wait(move || {
        let queue = queue;
        event_loop::detach_input(queue.0);
    });
}

unsafe extern "C" fn on_content_rect_changed(activity: *mut ANativeActivity, _: *const ARect) {
    if unsafe { update_insets(activity) } {
        android_post(window::resized);
    }
}

unsafe extern "C" fn on_configuration_changed(activity: *mut ANativeActivity) {
    let density_changed = unsafe { update_configuration(activity) };
    let insets_changed = unsafe { update_insets(activity) };
    android_post(move || window::configuration_changed(density_changed || insets_changed));
}

// MARK: Utils
const NIGHT_MODE_AUTO: jint = 0;
const NIGHT_MODE_NO: jint = 1;
const NIGHT_MODE_YES: jint = 2;
const ANDROID_S: i32 = 31;
const ANDROID_R: i32 = 30;

// The binary name, from the path of this library: lib<name>.so.
fn binary_name() -> CString {
    let mut info = Dl_info::default();
    let path = unsafe {
        dladdr(ANativeActivity_onCreate as *const c_void, &mut info);
        CStr::from_ptr(info.dli_fname).to_string_lossy()
    };
    let file = path.rsplit('/').next().unwrap_or_default();
    let name = file
        .strip_prefix("lib")
        .and_then(|file| file.strip_suffix(".so"))
        .unwrap_or(file);
    CString::new(name).unwrap_or_default()
}

/// Force the light or dark theme for this app, callable from every thread.
pub(super) fn set_dark_mode(dark: bool) {
    android_run_on_ui_thread(move || {
        set_night_mode(
            JniEnv::current(),
            if dark { NIGHT_MODE_YES } else { NIGHT_MODE_NO },
        );
    });
}

// The application night mode exists since Android 12, older versions follow the system.
fn set_night_mode(env: JniEnv, mode: jint) {
    let activity = ACTIVITY.load(Ordering::Acquire);
    if unsafe { (*activity).sdkVersion } < ANDROID_S {
        return;
    }
    unsafe {
        let name = env.new_string("uimode");
        let manager = env.call_object_method(
            android_activity(),
            c"getSystemService",
            c"(Ljava/lang/String;)Ljava/lang/Object;",
            &[jvalue { l: name }],
        );
        env.call_void_method(
            manager,
            c"setApplicationNightMode",
            c"(I)V",
            &[jvalue { i: mode }],
        );
        env.delete_local_ref(manager);
        env.delete_local_ref(name);
    }
}

unsafe fn window_and_decor_view(env: JniEnv) -> (jobject, jobject) {
    unsafe {
        let window = env.call_object_method(
            android_activity(),
            c"getWindow",
            c"()Landroid/view/Window;",
            &[],
        );
        let decor = env.call_object_method(window, c"getDecorView", c"()Landroid/view/View;", &[]);
        (window, decor)
    }
}

// Draw behind the system bars, the content is inset by update_insets.
unsafe fn draw_behind_system_bars(env: JniEnv, sdk: i32) {
    const LAYOUT_STABLE_FULLSCREEN_HIDE_NAVIGATION: jint = 0x700;
    unsafe {
        let (window, decor) = window_and_decor_view(env);
        if sdk >= ANDROID_R {
            env.call_void_method(
                window,
                c"setDecorFitsSystemWindows",
                c"(Z)V",
                &[jvalue { z: 0 }],
            );
        } else {
            env.call_void_method(
                decor,
                c"setSystemUiVisibility",
                c"(I)V",
                &[jvalue {
                    i: LAYOUT_STABLE_FULLSCREEN_HIDE_NAVIGATION,
                }],
            );
        }
        env.delete_local_ref(decor);
        env.delete_local_ref(window);
    }
}

/// Use light or dark system bar icons for a window background color, callable from every thread.
pub(super) fn set_system_bar_appearance(color: u32) {
    let [_, red, green, blue] = color.to_be_bytes().map(u32::from);
    let light = red * 299 + green * 587 + blue * 114 > 128_000;
    android_run_on_ui_thread(move || {
        const APPEARANCE_LIGHT_BARS: jint = 0x8 | 0x10;
        const LIGHT_STATUS_AND_NAVIGATION_BAR: jint = 0x2000 | 0x10;
        let env = JniEnv::current();
        let sdk = unsafe { (*ACTIVITY.load(Ordering::Acquire)).sdkVersion };
        unsafe {
            let (window, decor) = window_and_decor_view(env);
            if sdk >= ANDROID_R {
                let controller = env.call_object_method(
                    window,
                    c"getInsetsController",
                    c"()Landroid/view/WindowInsetsController;",
                    &[],
                );
                if !controller.is_null() {
                    env.call_void_method(
                        controller,
                        c"setSystemBarsAppearance",
                        c"(II)V",
                        &[
                            jvalue {
                                i: if light { APPEARANCE_LIGHT_BARS } else { 0 },
                            },
                            jvalue {
                                i: APPEARANCE_LIGHT_BARS,
                            },
                        ],
                    );
                }
            } else {
                let class = env.object_class(decor);
                let get = env.method(class, c"getSystemUiVisibility", c"()I");
                let visibility = env.call_int(decor, get, &[]);
                let visibility = if light {
                    visibility | LIGHT_STATUS_AND_NAVIGATION_BAR
                } else {
                    visibility & !LIGHT_STATUS_AND_NAVIGATION_BAR
                };
                env.call_void_method(
                    decor,
                    c"setSystemUiVisibility",
                    c"(I)V",
                    &[jvalue { i: visibility }],
                );
                env.delete_local_ref(class);
            }
            env.delete_local_ref(decor);
            env.delete_local_ref(window);
        }
    });
}

// Read the density, screen size, and night mode, returns whether the density changed.
// The activity configuration is current in onConfigurationChanged, unlike AConfiguration.
unsafe fn update_configuration(activity: *mut ANativeActivity) -> bool {
    const DENSITY_MEDIUM: f32 = 160.0;
    const UI_MODE_NIGHT_MASK: jint = 0x30;
    const UI_MODE_NIGHT_YES: jint = 0x20;
    unsafe {
        let env = JniEnv::from_raw((*activity).env);
        let resources = env.call_object_method(
            (*activity).clazz,
            c"getResources",
            c"()Landroid/content/res/Resources;",
            &[],
        );
        let config = env.call_object_method(
            resources,
            c"getConfiguration",
            c"()Landroid/content/res/Configuration;",
            &[],
        );
        let field = |name: &CStr| env.int_field(config, name).max(0) as u32;
        let density = field(c"densityDpi") as f32 / DENSITY_MEDIUM;
        DISPLAY.dark.store(
            env.int_field(config, c"uiMode") & UI_MODE_NIGHT_MASK == UI_MODE_NIGHT_YES,
            Ordering::Relaxed,
        );
        DISPLAY
            .screen_width_dp
            .store(field(c"screenWidthDp"), Ordering::Relaxed);
        DISPLAY
            .screen_height_dp
            .store(field(c"screenHeightDp"), Ordering::Relaxed);
        env.delete_local_ref(config);
        env.delete_local_ref(resources);
        DISPLAY.density.swap(density.to_bits(), Ordering::Relaxed) != density.to_bits()
    }
}

// Read the system bar and display cutout insets, returns whether they changed.
unsafe fn update_insets(activity: *mut ANativeActivity) -> bool {
    unsafe {
        let env = JniEnv::from_raw((*activity).env);
        let (window, decor) = window_and_decor_view(env);
        let insets = env.call_object_method(
            decor,
            c"getRootWindowInsets",
            c"()Landroid/view/WindowInsets;",
            &[],
        );
        let values = if insets.is_null() {
            [0; 4]
        } else if (*activity).sdkVersion >= ANDROID_R {
            let types = env.find_class(c"android/view/WindowInsets$Type");
            let type_mask = |name: &CStr| {
                let method = env.static_method(types, name, c"()I");
                env.call_static_int(types, method, &[])
            };
            let mask = type_mask(c"systemBars") | type_mask(c"displayCutout");
            let rect = env.call_object_method(
                insets,
                c"getInsets",
                c"(I)Landroid/graphics/Insets;",
                &[jvalue { i: mask }],
            );
            let values =
                [c"left", c"top", c"right", c"bottom"].map(|name| env.int_field(rect, name));
            env.delete_local_ref(rect);
            env.delete_local_ref(types);
            values
        } else {
            let class = env.object_class(insets);
            let values = [
                c"getSystemWindowInsetLeft",
                c"getSystemWindowInsetTop",
                c"getSystemWindowInsetRight",
                c"getSystemWindowInsetBottom",
            ]
            .map(|name| {
                let method = env.method(class, name, c"()I");
                env.call_int(insets, method, &[])
            });
            env.delete_local_ref(class);
            values
        };
        env.delete_local_ref(insets);
        env.delete_local_ref(decor);
        env.delete_local_ref(window);
        let mut changed = false;
        for (inset, value) in DISPLAY.insets.iter().zip(values) {
            changed |= inset.swap(value.max(0) as u32, Ordering::Relaxed) != value.max(0) as u32;
        }
        changed
    }
}
