/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::{CString, c_char, c_int, c_long, c_void};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::fd::FromRawFd;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use super::activity::android_activity;
use super::headers::*;
use super::jni::{GlobalRef, JniEnv};
use super::{input, window};
use crate::mailbox::{Mailbox, Message};
use crate::{EventLoopBuilder, LogicalPoint, LogicalSize, NativeEvent as Event, Theme};

// MARK: Shared state
// Written by the Android UI thread, read by the Rust main thread.
pub(super) struct Display {
    pub density: AtomicU32,
    pub window_width: AtomicU32,
    pub window_height: AtomicU32,
    pub insets: [AtomicU32; 4],
    pub screen_width_dp: AtomicU32,
    pub screen_height_dp: AtomicU32,
    pub dark: AtomicBool,
}

pub(super) static DISPLAY: Display = Display {
    density: AtomicU32::new(0x3f80_0000), // 1.0
    window_width: AtomicU32::new(0),
    window_height: AtomicU32::new(0),
    insets: [const { AtomicU32::new(0) }; 4],
    screen_width_dp: AtomicU32::new(0),
    screen_height_dp: AtomicU32::new(0),
    dark: AtomicBool::new(false),
};

/// Native pixels per logical pixel (the Android display density).
pub fn android_density() -> f32 {
    f32::from_bits(DISPLAY.density.load(Ordering::Relaxed))
}

/// The window content area inside the system bars, as `(x, y, width, height)` in native pixels.
pub fn android_content_rect() -> (u32, u32, u32, u32) {
    let [left, top, right, bottom] = DISPLAY
        .insets
        .each_ref()
        .map(|inset| inset.load(Ordering::Relaxed));
    let width = DISPLAY.window_width.load(Ordering::Relaxed);
    let height = DISPLAY.window_height.load(Ordering::Relaxed);
    (
        left,
        top,
        width.saturating_sub(left + right),
        height.saturating_sub(top + bottom),
    )
}

pub(super) fn content_size() -> LogicalSize {
    let (_, _, width, height) = android_content_rect();
    let density = android_density();
    LogicalSize::new(width as f32 / density, height as f32 / density)
}

pub(super) fn system_theme() -> Theme {
    if DISPLAY.dark.load(Ordering::Relaxed) {
        Theme::Dark
    } else {
        Theme::Light
    }
}

// The window's `android.view.Surface` and native window with its generation, only used on
// the Rust main thread.
thread_local! {
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static SURFACE: RefCell<Option<GlobalRef>> = const { RefCell::new(None) };
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static WINDOW: Cell<(*mut ANativeWindow, u64)> = const { Cell::new((null_mut(), 0)) };
}

/// The window's `android.view.Surface`, null while the window has no surface.
/// Must be called on the Rust main thread, the reference stays valid during the current task.
pub fn android_surface() -> jobject {
    SURFACE.with(|surface| {
        surface
            .borrow()
            .as_ref()
            .map_or(null_mut(), GlobalRef::as_raw)
    })
}

/// The window's `ANativeWindow`, null while the window has no surface, and a generation
/// that changes with every new native window. Must be called on the Rust main thread,
/// the native window stays valid during the current task.
pub fn android_native_window() -> (*mut c_void, u64) {
    let (window, generation) = WINDOW.get();
    (window.cast(), generation)
}

/// Use a new native window, or none when null.
pub(super) fn set_window(window: *mut ANativeWindow) {
    WINDOW.set((window, WINDOW.get().1 + 1));
    let env = JniEnv::current();
    let surface = (!window.is_null()).then(|| unsafe {
        update_window_size(window);
        GlobalRef::from_local(env, ANativeWindow_toSurface(env.as_raw(), window))
    });
    let previous = SURFACE.with(|current| current.replace(surface));
    if let Some(previous) = previous {
        unsafe { env.call_void_method(previous.as_raw(), c"release", c"()V", &[]) };
    }
}

/// Read the size of the current native window.
pub(super) fn update_window_size(window: *mut ANativeWindow) {
    unsafe {
        DISPLAY
            .window_width
            .store(ANativeWindow_getWidth(window) as u32, Ordering::Relaxed);
        DISPLAY
            .window_height
            .store(ANativeWindow_getHeight(window) as u32, Ordering::Relaxed);
    }
}

// MARK: Task queue
type Task = Box<dyn FnOnce() + Send>;

// A task queue served by a looper, woken through an eventfd.
pub(super) struct Queue {
    tasks: Mutex<VecDeque<Task>>,
    fd: AtomicI32,
}

impl Queue {
    pub(super) const fn new() -> Self {
        Self {
            tasks: Mutex::new(VecDeque::new()),
            fd: AtomicI32::new(-1),
        }
    }

    // Serve the queue from the looper of the current thread.
    pub(super) fn attach(&'static self) {
        unsafe {
            let fd = eventfd(0, EFD_CLOEXEC);
            assert!(fd >= 0, "can't create eventfd");
            self.fd.store(fd, Ordering::Release);
            ALooper_addFd(
                ALooper_forThread(),
                fd,
                ALOOPER_POLL_CALLBACK,
                ALOOPER_EVENT_INPUT,
                Self::wake,
                (self as *const Self).cast_mut().cast(),
            );
        }
        // Wake for tasks posted before the looper served the queue.
        self.notify();
    }

    pub(super) fn post(&self, task: Task) {
        self.tasks
            .lock()
            .expect("task queue poisoned")
            .push_back(task);
        self.notify();
    }

    fn notify(&self) {
        let fd = self.fd.load(Ordering::Acquire);
        if fd >= 0 {
            let value = 1u64;
            unsafe { write(fd, (&raw const value).cast(), size_of::<u64>()) };
        }
    }

    unsafe extern "C" fn wake(fd: c_int, _: c_int, data: *mut c_void) -> c_int {
        let queue = unsafe { &*data.cast::<Self>() };
        let mut value = 0u64;
        unsafe { read(fd, (&raw mut value).cast(), size_of::<u64>()) };
        let env = JniEnv::current();
        let tasks = std::mem::take(&mut *queue.tasks.lock().expect("task queue poisoned"));
        for task in tasks {
            env.with_local_frame(task);
        }
        1
    }
}

static QUEUE: Queue = Queue::new();

thread_local! {
    static LOCAL_TASKS: RefCell<VecDeque<Box<dyn FnOnce()>>> = const { RefCell::new(VecDeque::new()) };
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static EXIT_REQUESTED: Cell<bool> = const { Cell::new(false) };
}

/// Run `task` on the Rust main thread inside the event loop, callable from every thread.
pub fn android_post(task: impl FnOnce() + Send + 'static) {
    QUEUE.post(Box::new(task));
}

/// Run `task` on the Rust main thread and wait until it ran, for UI thread callbacks
/// that must not return before the Rust main thread stopped using a native resource.
pub(super) fn post_and_wait(task: impl FnOnce() + Send + 'static) {
    let done = Arc::new((Mutex::new(false), Condvar::new()));
    let signal = done.clone();
    android_post(move || {
        task();
        let (lock, ready) = &*signal;
        *lock.lock().expect("signal poisoned") = true;
        ready.notify_one();
    });
    let (lock, ready) = &*done;
    let mut finished = lock.lock().expect("signal poisoned");
    while !*finished {
        finished = ready.wait(finished).expect("signal poisoned");
    }
}

/// Run `task` later on the Rust main thread, after the current event, like an idle callback.
/// Must be called on the Rust main thread.
pub fn android_post_local(task: impl FnOnce() + 'static) {
    LOCAL_TASKS.with(|tasks| tasks.borrow_mut().push_back(Box::new(task)));
}

/// Run `task` on the Rust main thread at the next display frame.
/// Must be called on the Rust main thread.
pub fn android_post_frame(task: impl FnOnce() + 'static) {
    unsafe extern "C" fn frame(_: c_long, data: *mut c_void) {
        let task = unsafe { Box::from_raw(data.cast::<Box<dyn FnOnce()>>()) };
        JniEnv::current().with_local_frame(task);
    }
    let task: Box<Box<dyn FnOnce()>> = Box::new(Box::new(task));
    unsafe {
        AChoreographer_postFrameCallback(
            AChoreographer_getInstance(),
            frame,
            Box::into_raw(task).cast(),
        );
    }
}

pub(super) fn stop() {
    EXIT_REQUESTED.set(true);
}

// Serve the looper until a local task is pending or the loop should stop.
fn poll() {
    let env = JniEnv::current();
    while let Some(task) = LOCAL_TASKS.with(|tasks| tasks.borrow_mut().pop_front()) {
        env.with_local_frame(task);
        if EXIT_REQUESTED.get() {
            return;
        }
    }
    unsafe { ALooper_pollOnce(-1, null_mut(), null_mut(), null_mut()) };
}

// MARK: EventLoop
pub(crate) struct PlatformEventLoop {
    mailbox: Arc<Mailbox>,
}

impl PlatformEventLoop {
    pub(crate) fn new(_builder: EventLoopBuilder) -> Self {
        assert!(
            !android_activity().is_null(),
            "the event loop must be created by the main function the activity runs"
        );
        Self {
            mailbox: Arc::new(Mailbox::new()),
        }
    }
}

impl Drop for PlatformEventLoop {
    fn drop(&mut self) {
        crate::dispatch::shutdown(&self.mailbox);
        let local = LOCAL_TASKS.with(|tasks| std::mem::take(&mut *tasks.borrow_mut()));
        drop(local);
    }
}

impl crate::EventLoopInterface for PlatformEventLoop {
    fn theme(&self) -> Theme {
        system_theme()
    }

    fn primary_monitor(&self) -> PlatformMonitor {
        PlatformMonitor
    }

    fn available_monitors(&self) -> Vec<PlatformMonitor> {
        vec![PlatformMonitor]
    }

    fn run(self, event_handler: impl FnMut(Event) + 'static) -> i32 {
        crate::dispatch::start(event_handler);
        while !EXIT_REQUESTED.get() {
            poll();
        }
        // Deliver LoopExiting, then finish the activity, whose onDestroy ends the process.
        // Keep serving the looper, the UI thread waits for its window and input handoffs.
        drop(self);
        super::activity::finish();
        loop {
            poll();
        }
    }

    fn create_proxy(&self) -> PlatformEventLoopProxy {
        PlatformEventLoopProxy(self.mailbox.clone())
    }
}

pub(crate) use crate::dispatch::send as send_event;

// MARK: EventLoopProxy
pub(crate) struct PlatformEventLoopProxy(Arc<Mailbox>);

impl PlatformEventLoopProxy {
    fn send(&self, message: Message) -> bool {
        self.0.send(message, || {
            let mailbox = self.0.clone();
            android_post(move || {
                while let Some(message) = mailbox.pop() {
                    match message {
                        Message::User(value) => send_event(Event::UserEvent(value)),
                        Message::Exit => {
                            stop();
                            break;
                        }
                    }
                }
            });
            true
        })
    }
}

impl crate::EventLoopProxyInterface for PlatformEventLoopProxy {
    fn send_user_event(&self, data: Box<dyn std::any::Any + Send>) -> bool {
        self.send(Message::User(data))
    }

    fn exit(&self) -> bool {
        self.send(Message::Exit)
    }
}

// MARK: Monitor
pub(crate) struct PlatformMonitor;

impl crate::MonitorInterface for PlatformMonitor {
    fn name(&self) -> String {
        "Built-in display".to_string()
    }

    fn position(&self) -> LogicalPoint {
        LogicalPoint::new(0.0, 0.0)
    }

    fn size(&self) -> LogicalSize {
        LogicalSize::new(
            DISPLAY.screen_width_dp.load(Ordering::Relaxed) as f32,
            DISPLAY.screen_height_dp.load(Ordering::Relaxed) as f32,
        )
    }

    fn scale_factor(&self) -> f32 {
        android_density()
    }

    fn is_primary(&self) -> bool {
        true
    }
}

// MARK: Input
static INPUT_QUEUE: AtomicPtr<AInputQueue> = AtomicPtr::new(null_mut());

pub(super) fn attach_input(queue: *mut AInputQueue) {
    INPUT_QUEUE.store(queue, Ordering::Release);
    unsafe {
        AInputQueue_attachLooper(
            queue,
            ALooper_forThread(),
            ALOOPER_POLL_CALLBACK,
            read_input,
            null_mut(),
        );
    }
}

pub(super) fn detach_input(queue: *mut AInputQueue) {
    unsafe { AInputQueue_detachLooper(queue) };
    let _ = INPUT_QUEUE.compare_exchange(queue, null_mut(), Ordering::AcqRel, Ordering::Acquire);
}

unsafe extern "C" fn read_input(_: c_int, _: c_int, _: *mut c_void) -> c_int {
    let queue = INPUT_QUEUE.load(Ordering::Acquire);
    let env = JniEnv::current();
    let mut event = null_mut();
    while !queue.is_null() && unsafe { AInputQueue_getEvent(queue, &mut event) } >= 0 {
        // The input method gets the first chance to handle the event.
        if unsafe { AInputQueue_preDispatchEvent(queue, event) } != 0 {
            continue;
        }
        let handled = env.with_local_frame(|| unsafe { handle_input(env, event) });
        unsafe { AInputQueue_finishEvent(queue, event, c_int::from(handled)) };
    }
    1
}

unsafe fn handle_input(env: JniEnv, event: *mut AInputEvent) -> bool {
    unsafe {
        match AInputEvent_getType(event) {
            AINPUT_EVENT_TYPE_MOTION => {
                let (left, top, _, _) = android_content_rect();
                let density = android_density();
                let position = LogicalPoint::new(
                    (AMotionEvent_getX(event, 0) - left as f32) / density,
                    (AMotionEvent_getY(event, 0) - top as f32) / density,
                );
                let mouse = AMotionEvent_getToolType(event, 0) == AMOTION_EVENT_TOOL_TYPE_MOUSE;
                let scroll = (
                    AMotionEvent_getAxisValue(event, AMOTION_EVENT_AXIS_HSCROLL, 0),
                    -AMotionEvent_getAxisValue(event, AMOTION_EVENT_AXIS_VSCROLL, 0),
                );
                let events = input::motion_events(
                    AMotionEvent_getAction(event) & AMOTION_EVENT_ACTION_MASK,
                    position,
                    mouse,
                    scroll,
                );
                let handled = !events.is_empty();
                for event in events {
                    window::send(event);
                }
                handled
            }
            AINPUT_EVENT_TYPE_KEY => {
                let action = AKeyEvent_getAction(event);
                let key_code = AKeyEvent_getKeyCode(event);
                if key_code == AKEYCODE_BACK {
                    if action == AKEY_EVENT_ACTION_UP {
                        window::close_requested();
                    }
                    return true;
                }
                if action != AKEY_EVENT_ACTION_DOWN && action != AKEY_EVENT_ACTION_UP {
                    return false;
                }
                let meta_state = AKeyEvent_getMetaState(event);
                window::send(input::key_event(
                    action == AKEY_EVENT_ACTION_DOWN,
                    key_code,
                    unicode_char(env, AInputEvent_getDeviceId(event), key_code, meta_state),
                    AKeyEvent_getRepeatCount(event),
                    meta_state,
                ));
                true
            }
            _ => false,
        }
    }
}

// The NDK has no character mapping, so ask the device's android.view.KeyCharacterMap.
// KeyCharacterMap.load throws for removed devices, InputDevice.getDevice returns null instead.
unsafe fn unicode_char(env: JniEnv, device: i32, key_code: i32, meta_state: i32) -> i32 {
    const META_SHIFT_MASK: i32 = 0xc1;
    const META_CAPS_LOCK_ON: i32 = 0x0010_0000;
    const VIRTUAL_KEYBOARD: i32 = -1;
    unsafe {
        let input_device = env.find_class(c"android/view/InputDevice");
        let get_device =
            env.static_method(input_device, c"getDevice", c"(I)Landroid/view/InputDevice;");
        let input_device =
            env.call_static_object(input_device, get_device, &[jvalue { i: device }]);
        let class = env.find_class(c"android/view/KeyCharacterMap");
        let map = if input_device.is_null() {
            let load = env.static_method(class, c"load", c"(I)Landroid/view/KeyCharacterMap;");
            env.call_static_object(
                class,
                load,
                &[jvalue {
                    i: VIRTUAL_KEYBOARD,
                }],
            )
        } else {
            env.call_object_method(
                input_device,
                c"getKeyCharacterMap",
                c"()Landroid/view/KeyCharacterMap;",
                &[],
            )
        };
        let get = env.method(class, c"get", c"(II)I");
        env.call_int(
            map,
            get,
            &[
                jvalue { i: key_code },
                jvalue {
                    i: meta_state & (META_SHIFT_MASK | META_CAPS_LOCK_ON),
                },
            ],
        )
    }
}

// MARK: Main thread
unsafe extern "C" {
    // The C entry point rustc generates for the application binary, linked as a shared library.
    fn main(argc: c_int, argv: *const *const c_char) -> c_int;
}

/// Run the application's main function on a dedicated "main" thread with its own looper.
pub(super) fn start(name: CString) {
    std::thread::Builder::new()
        .name("main".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            unsafe { ALooper_prepare(ALOOPER_PREPARE_ALLOW_NON_CALLBACKS) };
            QUEUE.attach();
            redirect_output_to_logcat(name.clone());
            let argv = [name.as_ptr(), null()];
            unsafe { main(1, argv.as_ptr()) };
            // main returned without running the event loop, or panicked, so close the activity.
            // Keep serving the looper, the UI thread waits for its window and input handoffs.
            super::activity::finish();
            loop {
                poll();
            }
        })
        .expect("can't start the main thread");
}

// Android discards stdout and stderr, forward them and panics to logcat instead.
fn redirect_output_to_logcat(tag: CString) {
    std::panic::set_hook(Box::new({
        let tag = tag.clone();
        move |info| {
            let message = CString::new(info.to_string().replace('\0', "")).unwrap_or_default();
            unsafe { __android_log_write(ANDROID_LOG_ERROR, tag.as_ptr(), message.as_ptr()) };
        }
    }));

    let mut fds: [c_int; 2] = [0; 2];
    unsafe {
        if pipe(fds.as_mut_ptr()) != 0 || dup2(fds[1], 1) < 0 || dup2(fds[1], 2) < 0 {
            return;
        }
    }
    let reader = BufReader::new(unsafe { File::from_raw_fd(fds[0]) });
    std::thread::spawn(move || {
        for line in reader.split(b'\n').map_while(Result::ok) {
            let message = CString::new(
                line.into_iter()
                    .filter(|byte| *byte != 0)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default();
            unsafe { __android_log_write(ANDROID_LOG_INFO, tag.as_ptr(), message.as_ptr()) };
        }
    });
}
