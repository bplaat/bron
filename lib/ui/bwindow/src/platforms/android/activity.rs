/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, CString, c_int, c_void};
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use jni::errors::Result;
use jni::objects::{Global, JObject};
use jni::sys::jint;
use jni::vm::JavaVM;
use jni::{Env, JValue, jni_sig, jni_str};

use super::event_loop::{self, DISPLAY, Queue, android_post, post_and_wait};
use super::headers::*;
use super::window;

// The framework android.app.NativeActivity hosts the single window, its callbacks run on
// the Android UI thread. The process runs one Rust main and exits with the activity.
static ACTIVITY: AtomicPtr<ANativeActivity> = AtomicPtr::new(null_mut());
pub(super) static ACTIVITY_OBJECT: OnceLock<Global<JObject<'static>>> = OnceLock::new();
static UI_QUEUE: Queue = Queue::new();

/// The running `android.app.NativeActivity`.
pub fn android_activity() -> &'static JObject<'static> {
    ACTIVITY_OBJECT
        .get()
        .expect("the activity is not created")
        .as_obj()
}

/// Run `f` with the JNI environment of the current thread in a new local reference frame,
/// attaching the thread when needed. Java exceptions and JNI errors panic.
pub fn android_with_env<T>(f: impl FnOnce(&mut Env) -> Result<T>) -> T {
    JavaVM::singleton()
        .and_then(|vm| vm.attach_current_thread(f))
        .expect("JNI call failed")
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

fn sdk_version() -> i32 {
    unsafe { (*ACTIVITY.load(Ordering::Acquire)).sdkVersion }
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

        JavaVM::from_raw((*activity).vm);
        let clazz = (*activity).clazz;
        let object = android_with_env(|env| env.new_global_ref(JObject::from_raw(env, clazz)));
        let _ = ACTIVITY_OBJECT.set(object);
        ACTIVITY.store(activity, Ordering::Release);
        UI_QUEUE.attach();

        // NativeActivity defaults to a 16-bit RGB_565 window.
        ANativeActivity_setWindowFormat(activity, WINDOW_FORMAT_RGBA_8888);
    }
    // Follow the system theme until the app calls set_theme, also after a crash.
    set_night_mode(NIGHT_MODE_AUTO);
    draw_behind_system_bars();
    update_configuration();
    event_loop::start(binary_name());
}

// MARK: Callbacks
unsafe extern "C" fn on_destroy(activity: *mut ANativeActivity) {
    if activity != ACTIVITY.load(Ordering::Acquire) {
        return;
    }
    // The system persists the application night mode, don't leak set_theme into the next launch.
    set_night_mode(NIGHT_MODE_AUTO);
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

unsafe extern "C" fn on_content_rect_changed(_: *mut ANativeActivity, _: *const ARect) {
    if update_insets() {
        android_post(window::safe_area_changed);
    }
}

unsafe extern "C" fn on_configuration_changed(_: *mut ANativeActivity) {
    let density_changed = update_configuration();
    let insets_changed = update_insets();
    // The insets are in dp, so a density change also changes them.
    android_post(move || {
        window::configuration_changed(density_changed, density_changed || insets_changed);
    });
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
        set_night_mode(if dark { NIGHT_MODE_YES } else { NIGHT_MODE_NO });
    });
}

// The application night mode exists since Android 12, older versions follow the system.
fn set_night_mode(mode: jint) {
    if sdk_version() < ANDROID_S {
        return;
    }
    android_with_env(|env| {
        let name = env.new_string("uimode")?;
        let manager = env
            .call_method(
                android_activity(),
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[(&name).into()],
            )?
            .l()?;
        env.call_method(
            &manager,
            jni_str!("setApplicationNightMode"),
            jni_sig!("(I)V"),
            &[JValue::Int(mode)],
        )?
        .v()
    });
}

fn window_and_decor_view<'local>(
    env: &mut Env<'local>,
) -> Result<(JObject<'local>, JObject<'local>)> {
    let window = env
        .call_method(
            android_activity(),
            jni_str!("getWindow"),
            jni_sig!("()Landroid/view/Window;"),
            &[],
        )?
        .l()?;
    let decor = env
        .call_method(
            &window,
            jni_str!("getDecorView"),
            jni_sig!("()Landroid/view/View;"),
            &[],
        )?
        .l()?;
    Ok((window, decor))
}

// Draw behind the system bars, update_insets reads the safe area.
fn draw_behind_system_bars() {
    const LAYOUT_STABLE_FULLSCREEN_HIDE_NAVIGATION: jint = 0x700;
    android_with_env(|env| {
        let (window, decor) = window_and_decor_view(env)?;
        if sdk_version() >= ANDROID_R {
            env.call_method(
                &window,
                jni_str!("setDecorFitsSystemWindows"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(false)],
            )?
            .v()
        } else {
            env.call_method(
                &decor,
                jni_str!("setSystemUiVisibility"),
                jni_sig!("(I)V"),
                &[JValue::Int(LAYOUT_STABLE_FULLSCREEN_HIDE_NAVIGATION)],
            )?
            .v()
        }
    });
}

/// Use light or dark system bar icons for a window background color, callable from every thread.
pub(super) fn set_system_bar_appearance(color: u32) {
    let [_, red, green, blue] = color.to_be_bytes().map(u32::from);
    let light = red * 299 + green * 587 + blue * 114 > 128_000;
    android_run_on_ui_thread(move || {
        const APPEARANCE_LIGHT_BARS: jint = 0x8 | 0x10;
        const LIGHT_STATUS_AND_NAVIGATION_BAR: jint = 0x2000 | 0x10;
        android_with_env(|env| {
            let (window, decor) = window_and_decor_view(env)?;
            if sdk_version() >= ANDROID_R {
                let controller = env
                    .call_method(
                        &window,
                        jni_str!("getInsetsController"),
                        jni_sig!("()Landroid/view/WindowInsetsController;"),
                        &[],
                    )?
                    .l()?;
                if controller.is_null() {
                    return Ok(());
                }
                env.call_method(
                    &controller,
                    jni_str!("setSystemBarsAppearance"),
                    jni_sig!("(II)V"),
                    &[
                        JValue::Int(if light { APPEARANCE_LIGHT_BARS } else { 0 }),
                        JValue::Int(APPEARANCE_LIGHT_BARS),
                    ],
                )?
                .v()
            } else {
                let visibility = env
                    .call_method(
                        &decor,
                        jni_str!("getSystemUiVisibility"),
                        jni_sig!("()I"),
                        &[],
                    )?
                    .i()?;
                let visibility = if light {
                    visibility | LIGHT_STATUS_AND_NAVIGATION_BAR
                } else {
                    visibility & !LIGHT_STATUS_AND_NAVIGATION_BAR
                };
                env.call_method(
                    &decor,
                    jni_str!("setSystemUiVisibility"),
                    jni_sig!("(I)V"),
                    &[JValue::Int(visibility)],
                )?
                .v()
            }
        });
    });
}

// Read the density, screen size, and night mode, returns whether the density changed.
// The activity configuration is current in onConfigurationChanged, unlike AConfiguration.
fn update_configuration() -> bool {
    const DENSITY_MEDIUM: f32 = 160.0;
    const UI_MODE_NIGHT_MASK: jint = 0x30;
    const UI_MODE_NIGHT_YES: jint = 0x20;
    let [density_dpi, ui_mode, screen_width_dp, screen_height_dp] = android_with_env(|env| {
        let resources = env
            .call_method(
                android_activity(),
                jni_str!("getResources"),
                jni_sig!("()Landroid/content/res/Resources;"),
                &[],
            )?
            .l()?;
        let config = env
            .call_method(
                &resources,
                jni_str!("getConfiguration"),
                jni_sig!("()Landroid/content/res/Configuration;"),
                &[],
            )?
            .l()?;
        let mut values = [0; 4];
        for (value, name) in values.iter_mut().zip([
            jni_str!("densityDpi"),
            jni_str!("uiMode"),
            jni_str!("screenWidthDp"),
            jni_str!("screenHeightDp"),
        ]) {
            *value = env.get_field(&config, name, jni_sig!("I"))?.i()?;
        }
        Ok(values)
    });
    let density = density_dpi.max(0) as f32 / DENSITY_MEDIUM;
    DISPLAY.dark.store(
        ui_mode & UI_MODE_NIGHT_MASK == UI_MODE_NIGHT_YES,
        Ordering::Relaxed,
    );
    DISPLAY
        .screen_width_dp
        .store(screen_width_dp.max(0) as u32, Ordering::Relaxed);
    DISPLAY
        .screen_height_dp
        .store(screen_height_dp.max(0) as u32, Ordering::Relaxed);
    DISPLAY.density.swap(density.to_bits(), Ordering::Relaxed) != density.to_bits()
}

// Read the system bar and display cutout insets, returns whether they changed.
fn update_insets() -> bool {
    let values = android_with_env(|env| {
        let (_, decor) = window_and_decor_view(env)?;
        let insets = env
            .call_method(
                &decor,
                jni_str!("getRootWindowInsets"),
                jni_sig!("()Landroid/view/WindowInsets;"),
                &[],
            )?
            .l()?;
        let mut values = [0; 4];
        if insets.is_null() {
            return Ok(values);
        }
        if sdk_version() >= ANDROID_R {
            let types = env.find_class(jni_str!("android/view/WindowInsets$Type"))?;
            let mut mask = 0;
            for name in [jni_str!("systemBars"), jni_str!("displayCutout")] {
                mask |= env
                    .call_static_method(&types, name, jni_sig!("()I"), &[])?
                    .i()?;
            }
            let rect = env
                .call_method(
                    &insets,
                    jni_str!("getInsets"),
                    jni_sig!("(I)Landroid/graphics/Insets;"),
                    &[JValue::Int(mask)],
                )?
                .l()?;
            for (value, name) in values.iter_mut().zip([
                jni_str!("left"),
                jni_str!("top"),
                jni_str!("right"),
                jni_str!("bottom"),
            ]) {
                *value = env.get_field(&rect, name, jni_sig!("I"))?.i()?;
            }
        } else {
            for (value, name) in values.iter_mut().zip([
                jni_str!("getSystemWindowInsetLeft"),
                jni_str!("getSystemWindowInsetTop"),
                jni_str!("getSystemWindowInsetRight"),
                jni_str!("getSystemWindowInsetBottom"),
            ]) {
                *value = env.call_method(&insets, name, jni_sig!("()I"), &[])?.i()?;
            }
        }
        Ok(values)
    });
    let mut changed = false;
    for (inset, value) in DISPLAY.insets.iter().zip(values) {
        changed |= inset.swap(value.max(0) as u32, Ordering::Relaxed) != value.max(0) as u32;
    }
    changed
}
