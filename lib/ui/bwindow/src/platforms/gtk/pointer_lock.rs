/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The relative pointer motion of a pointer lock. X11 keeps the hidden pointer in the middle
//! of the content and reports how far it moves from there. Wayland cannot move the pointer,
//! so it locks it in place with the pointer constraints protocol and reports the relative
//! pointer motion. Without those protocols the motion of the grabbed pointer is reported.

use std::cell::RefCell;
use std::ffi::c_void;

use super::headers::*;
use super::input::gtk_input_event;
use crate::{LogicalPoint, WindowEvent, WindowEventSender};

const GDK_MOTION_NOTIFY: i32 = 3;

unsafe extern "C" {
    fn gdk_seat_get_pointer(seat: *mut c_void) -> *mut c_void;
    fn gdk_window_get_screen(window: *mut c_void) -> *mut c_void;
    fn gdk_window_get_root_coords(window: *mut c_void, x: i32, y: i32, rx: *mut i32, ry: *mut i32);
    fn gdk_event_get_root_coords(event: *const GdkEvent, x: *mut f64, y: *mut f64) -> i32;
    fn gdk_device_warp(device: *mut c_void, screen: *mut c_void, x: i32, y: i32);
}

enum Lock {
    // The pointer is moved back to `center` in root coordinates after every motion.
    Warp {
        device: *mut c_void,
        screen: *mut c_void,
        center: (i32, i32),
    },
    // Destroying the lock releases the pointer.
    Wayland {
        _lock: wayland::Lock,
    },
    // The pointer moves freely, from `last` in window coordinates.
    Grab {
        last: Option<(f64, f64)>,
    },
}

thread_local! {
    static LOCK: RefCell<Option<Lock>> = const { RefCell::new(None) };
}

/// Start reporting relative motion over `widget` for the grabbed pointer of `seat`.
pub(super) unsafe fn start(
    display: *mut GdkDisplay,
    seat: *mut c_void,
    widget: *mut GtkWidget,
    sender: &WindowEventSender,
) {
    stop();
    let window = unsafe { gtk_widget_get_window(widget) };
    let device = unsafe { gdk_seat_get_pointer(seat) };
    if unsafe { gdk_display_is_wayland(display) } {
        let lock = unsafe { wayland::Lock::new(display, device, window, sender) };
        LOCK.set(Some(lock.map_or(Lock::Grab { last: None }, |lock| {
            Lock::Wayland { _lock: lock }
        })));
        return;
    }
    let (mut x, mut y) = (0, 0);
    unsafe {
        gdk_window_get_root_coords(
            window,
            gtk_widget_get_allocated_width(widget) / 2,
            gtk_widget_get_allocated_height(widget) / 2,
            &mut x,
            &mut y,
        );
    }
    let screen = unsafe { gdk_window_get_screen(window) };
    unsafe { gdk_device_warp(device, screen, x, y) };
    LOCK.set(Some(Lock::Warp {
        device,
        screen,
        center: (x, y),
    }));
}

/// Stop reporting relative motion.
pub(super) fn stop() {
    LOCK.set(None);
}

/// Report the motion of a locked pointer, returns whether `event` was handled.
pub(super) unsafe fn motion(event: *const GdkEvent, sender: &WindowEventSender) -> bool {
    if unsafe { gdk_event_get_event_type(event) } != GDK_MOTION_NOTIFY {
        return false;
    }
    LOCK.with_borrow_mut(|lock| {
        let movement = match lock {
            None => return false,
            Some(Lock::Warp {
                device,
                screen,
                center,
            }) => {
                let (mut x, mut y) = (0.0, 0.0);
                unsafe { gdk_event_get_root_coords(event, &mut x, &mut y) };
                let movement = (x - f64::from(center.0), y - f64::from(center.1));
                if movement == (0.0, 0.0) {
                    // The motion of moving the pointer back.
                    return true;
                }
                unsafe { gdk_device_warp(*device, *screen, center.0, center.1) };
                movement
            }
            // The relative pointer reports the motion of the locked pointer.
            Some(Lock::Wayland { .. }) => return true,
            Some(Lock::Grab { last }) => {
                let (mut x, mut y) = (0.0, 0.0);
                unsafe { gdk_event_get_coords(event, &mut x, &mut y) };
                let movement = last.map_or((0.0, 0.0), |(lx, ly)| (x - lx, y - ly));
                *last = Some((x, y));
                movement
            }
        };
        if let Some(WindowEvent::MouseMove {
            position,
            modifiers,
            ..
        }) = unsafe { gtk_input_event(event.cast(), false) }
        {
            sender.send(WindowEvent::MouseMove {
                position,
                movement: LogicalPoint::new(movement.0 as f32, movement.1 as f32),
                modifiers,
            });
        }
        true
    })
}

mod wayland {
    //! The `zwp_pointer_constraints_v1` and `zwp_relative_pointer_manager_v1` protocols,
    //! with the interface definitions their generated C code would have. Like everything
    //! else the GTK backend uses, libwayland-client is GDK's, so its functions and the GDK
    //! Wayland backend functions are looked up in the process instead of being linked.

    use std::cell::{Cell, OnceCell, UnsafeCell};
    use std::ffi::{CStr, c_char, c_void};
    use std::mem::transmute;
    use std::ptr::{null, null_mut};

    use super::super::headers::GdkDisplay;
    use crate::{LogicalPoint, Modifiers, WindowEvent, WindowEventSender};

    const RTLD_LAZY: i32 = 1;
    const LIFETIME_PERSISTENT: u32 = 2;

    // The BSDs provide the dynamic loader functions in libc.
    #[cfg_attr(target_os = "linux", link(name = "dl"))]
    unsafe extern "C" {
        fn dlopen(path: *const c_char, flags: i32) -> *mut c_void;
        fn dlsym(library: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    #[repr(C)]
    struct Interface {
        name: *const c_char,
        version: i32,
        method_count: i32,
        methods: *const Message,
        event_count: i32,
        events: *const Message,
    }

    #[repr(C)]
    struct Message {
        name: *const c_char,
        signature: *const c_char,
        types: *const *const Interface,
    }

    // SAFETY: The definitions are immutable and only point to other statics.
    unsafe impl Sync for Interface {}
    // SAFETY: See `Interface`.
    unsafe impl Sync for Message {}

    // Argument interfaces, the core Wayland ones are filled in when libwayland-client is found.
    #[repr(transparent)]
    struct Types<const N: usize>(UnsafeCell<[*const Interface; N]>);
    // SAFETY: The types are only written once on the UI thread, before any request uses them.
    unsafe impl<const N: usize> Sync for Types<N> {}

    impl<const N: usize> Types<N> {
        const fn new(types: [*const Interface; N]) -> Self {
            Self(UnsafeCell::new(types))
        }
        const fn as_ptr(&self) -> *const *const Interface {
            self.0.get().cast()
        }
    }

    static NO_TYPES: Types<6> = Types::new([null(); 6]);

    const fn message(
        name: &'static CStr,
        signature: &'static CStr,
        types: *const *const Interface,
    ) -> Message {
        Message {
            name: name.as_ptr(),
            signature: signature.as_ptr(),
            types,
        }
    }

    // `wl_pointer`.
    static RELATIVE_POINTER_MANAGER_TYPES: Types<2> =
        Types::new([&raw const RELATIVE_POINTER_INTERFACE, null()]);
    static RELATIVE_POINTER_MANAGER_METHODS: [Message; 2] = [
        message(c"destroy", c"", NO_TYPES.as_ptr()),
        message(
            c"get_relative_pointer",
            c"no",
            RELATIVE_POINTER_MANAGER_TYPES.as_ptr(),
        ),
    ];
    static RELATIVE_POINTER_MANAGER_INTERFACE: Interface = Interface {
        name: c"zwp_relative_pointer_manager_v1".as_ptr(),
        version: 1,
        method_count: 2,
        methods: RELATIVE_POINTER_MANAGER_METHODS.as_ptr(),
        event_count: 0,
        events: null(),
    };

    static RELATIVE_POINTER_METHODS: [Message; 1] = [message(c"destroy", c"", NO_TYPES.as_ptr())];
    static RELATIVE_POINTER_EVENTS: [Message; 1] =
        [message(c"relative_motion", c"uuffff", NO_TYPES.as_ptr())];
    static RELATIVE_POINTER_INTERFACE: Interface = Interface {
        name: c"zwp_relative_pointer_v1".as_ptr(),
        version: 1,
        method_count: 1,
        methods: RELATIVE_POINTER_METHODS.as_ptr(),
        event_count: 1,
        events: RELATIVE_POINTER_EVENTS.as_ptr(),
    };

    // `wl_surface`, `wl_pointer` and `wl_region`. Confining the pointer, the third method, is
    // never requested.
    static POINTER_CONSTRAINTS_TYPES: Types<5> = Types::new([
        &raw const LOCKED_POINTER_INTERFACE,
        null(),
        null(),
        null(),
        null(),
    ]);
    static POINTER_CONSTRAINTS_METHODS: [Message; 2] = [
        message(c"destroy", c"", NO_TYPES.as_ptr()),
        message(
            c"lock_pointer",
            c"noo?ou",
            POINTER_CONSTRAINTS_TYPES.as_ptr(),
        ),
    ];
    static POINTER_CONSTRAINTS_INTERFACE: Interface = Interface {
        name: c"zwp_pointer_constraints_v1".as_ptr(),
        version: 1,
        method_count: 2,
        methods: POINTER_CONSTRAINTS_METHODS.as_ptr(),
        event_count: 0,
        events: null(),
    };

    static LOCKED_POINTER_METHODS: [Message; 1] = [message(c"destroy", c"", NO_TYPES.as_ptr())];
    static LOCKED_POINTER_EVENTS: [Message; 2] = [
        message(c"locked", c"", NO_TYPES.as_ptr()),
        message(c"unlocked", c"", NO_TYPES.as_ptr()),
    ];
    static LOCKED_POINTER_INTERFACE: Interface = Interface {
        name: c"zwp_locked_pointer_v1".as_ptr(),
        version: 1,
        method_count: 1,
        methods: LOCKED_POINTER_METHODS.as_ptr(),
        event_count: 2,
        events: LOCKED_POINTER_EVENTS.as_ptr(),
    };

    type Marshal = unsafe extern "C" fn(*mut c_void, u32, ...);
    type Constructor = unsafe extern "C" fn(*mut c_void, u32, *const Interface, ...) -> *mut c_void;
    type ConstructorVersioned =
        unsafe extern "C" fn(*mut c_void, u32, *const Interface, u32, ...) -> *mut c_void;
    type AddListener = unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) -> i32;
    type Proxy = unsafe extern "C" fn(*mut c_void);
    type Display = unsafe extern "C" fn(*mut c_void) -> i32;
    type Handle<T> = unsafe extern "C" fn(*mut T) -> *mut c_void;

    // The libwayland-client and GDK Wayland functions in the process.
    #[derive(Clone, Copy)]
    struct Api {
        marshal: Marshal,
        constructor: Constructor,
        constructor_versioned: ConstructorVersioned,
        add_listener: AddListener,
        destroy: Proxy,
        roundtrip: Display,
        flush: Display,
        registry_interface: *const Interface,
        wl_display: Handle<GdkDisplay>,
        wl_pointer: Handle<c_void>,
        wl_surface: Handle<c_void>,
    }

    thread_local! {
        static API: OnceCell<Option<Api>> = const { OnceCell::new() };
        static GLOBALS: Cell<Option<Globals>> = const { Cell::new(None) };
    }

    fn api() -> Option<Api> {
        API.with(|api| *api.get_or_init(|| unsafe { load() }))
    }

    unsafe fn load() -> Option<Api> {
        // SAFETY: The process handle finds the symbols of the libraries GDK loaded, which stay
        // loaded, and each symbol has the type of its C declaration.
        unsafe {
            let process = dlopen(null(), RTLD_LAZY);
            let symbol = |name: &CStr| {
                let symbol = dlsym(process, name.as_ptr());
                (!symbol.is_null()).then_some(symbol)
            };
            let api = Api {
                marshal: transmute::<*mut c_void, Marshal>(symbol(c"wl_proxy_marshal")?),
                constructor: transmute::<*mut c_void, Constructor>(symbol(
                    c"wl_proxy_marshal_constructor",
                )?),
                constructor_versioned: transmute::<*mut c_void, ConstructorVersioned>(symbol(
                    c"wl_proxy_marshal_constructor_versioned",
                )?),
                add_listener: transmute::<*mut c_void, AddListener>(symbol(
                    c"wl_proxy_add_listener",
                )?),
                destroy: transmute::<*mut c_void, Proxy>(symbol(c"wl_proxy_destroy")?),
                roundtrip: transmute::<*mut c_void, Display>(symbol(c"wl_display_roundtrip")?),
                flush: transmute::<*mut c_void, Display>(symbol(c"wl_display_flush")?),
                registry_interface: symbol(c"wl_registry_interface")?.cast(),
                wl_display: transmute::<*mut c_void, Handle<GdkDisplay>>(symbol(
                    c"gdk_wayland_display_get_wl_display",
                )?),
                wl_pointer: transmute::<*mut c_void, Handle<c_void>>(symbol(
                    c"gdk_wayland_device_get_wl_pointer",
                )?),
                wl_surface: transmute::<*mut c_void, Handle<c_void>>(symbol(
                    c"gdk_wayland_window_get_wl_surface",
                )?),
            };
            let pointer = symbol(c"wl_pointer_interface")?.cast();
            (*RELATIVE_POINTER_MANAGER_TYPES.0.get())[1] = pointer;
            let types = &mut *POINTER_CONSTRAINTS_TYPES.0.get();
            types[1] = symbol(c"wl_surface_interface")?.cast();
            types[2] = pointer;
            types[3] = symbol(c"wl_region_interface")?.cast();
            Some(api)
        }
    }

    #[repr(C)]
    struct RegistryListener {
        global: extern "C" fn(*mut Globals, *mut c_void, u32, *const c_char, u32),
        global_remove: extern "C" fn(*mut Globals, *mut c_void, u32),
    }

    #[repr(C)]
    struct RelativePointerListener {
        relative_motion:
            extern "C" fn(*const WindowEventSender, *mut c_void, u32, u32, i32, i32, i32, i32),
    }

    static REGISTRY_LISTENER: RegistryListener = RegistryListener {
        global: registry_global,
        global_remove: registry_global_remove,
    };

    static RELATIVE_POINTER_LISTENER: RelativePointerListener =
        RelativePointerListener { relative_motion };

    // The protocol globals of the display, bound once.
    #[derive(Clone, Copy)]
    struct Globals {
        constraints: *mut c_void,
        relative: *mut c_void,
    }

    extern "C" fn registry_global(
        globals: *mut Globals,
        registry: *mut c_void,
        name: u32,
        interface: *const c_char,
        _version: u32,
    ) {
        let Some(api) = api() else {
            return;
        };
        // SAFETY: The registry passes a valid interface name and the globals of `globals`.
        unsafe {
            let globals = &mut *globals;
            let interface = CStr::from_ptr(interface);
            let (target, definition) = if interface == c"zwp_pointer_constraints_v1" {
                (
                    &mut globals.constraints,
                    &raw const POINTER_CONSTRAINTS_INTERFACE,
                )
            } else if interface == c"zwp_relative_pointer_manager_v1" {
                (
                    &mut globals.relative,
                    &raw const RELATIVE_POINTER_MANAGER_INTERFACE,
                )
            } else {
                return;
            };
            *target = (api.constructor_versioned)(
                registry,
                0, // wl_registry.bind
                definition,
                1,
                name,
                (*definition).name,
                1u32,
                null_mut::<c_void>(),
            );
        }
    }

    const extern "C" fn registry_global_remove(_: *mut Globals, _: *mut c_void, _: u32) {}

    extern "C" fn relative_motion(
        sender: *const WindowEventSender,
        _: *mut c_void,
        _: u32,
        _: u32,
        dx: i32,
        dy: i32,
        _: i32,
        _: i32,
    ) {
        // The locked pointer stays in place, the window keeps its last position.
        // SAFETY: The lock owns the sender until it destroys the relative pointer.
        unsafe { &*sender }.send(WindowEvent::MouseMove {
            position: LogicalPoint::default(),
            movement: LogicalPoint::new(dx as f32 / 256.0, dy as f32 / 256.0),
            modifiers: Modifiers::default(),
        });
    }

    // Binds the protocol globals of the display, when it has both.
    unsafe fn globals(api: Api, display: *mut c_void) -> Option<Globals> {
        if let Some(globals) = GLOBALS.get() {
            return Some(globals);
        }
        let mut globals = Globals {
            constraints: null_mut(),
            relative: null_mut(),
        };
        // SAFETY: The display is live, the registry is only used during the roundtrip.
        unsafe {
            let registry = (api.constructor)(
                display,
                1, // wl_display.get_registry
                api.registry_interface,
                null_mut::<c_void>(),
            );
            (api.add_listener)(
                registry,
                (&raw const REGISTRY_LISTENER).cast(),
                (&raw mut globals).cast(),
            );
            (api.roundtrip)(display);
            (api.destroy)(registry);
        }
        if globals.constraints.is_null() || globals.relative.is_null() {
            return None;
        }
        GLOBALS.set(Some(globals));
        Some(globals)
    }

    /// A locked pointer and its relative motion.
    pub(super) struct Lock {
        api: Api,
        display: *mut c_void,
        locked: *mut c_void,
        relative: *mut c_void,
        _sender: Box<WindowEventSender>,
    }

    impl Lock {
        pub(super) unsafe fn new(
            display: *mut GdkDisplay,
            device: *mut c_void,
            window: *mut c_void,
            sender: &WindowEventSender,
        ) -> Option<Self> {
            let api = api()?;
            // SAFETY: The caller supplies a live Wayland display, pointer and window.
            unsafe {
                let display = (api.wl_display)(display);
                let pointer = (api.wl_pointer)(device);
                let surface = (api.wl_surface)(window);
                if pointer.is_null() || surface.is_null() {
                    return None;
                }
                let globals = globals(api, display)?;
                let sender = Box::new(sender.clone());
                let relative = (api.constructor)(
                    globals.relative,
                    1, // get_relative_pointer
                    &raw const RELATIVE_POINTER_INTERFACE,
                    null_mut::<c_void>(),
                    pointer,
                );
                (api.add_listener)(
                    relative,
                    (&raw const RELATIVE_POINTER_LISTENER).cast(),
                    (&raw const *sender).cast_mut().cast(),
                );
                let locked = (api.constructor)(
                    globals.constraints,
                    1, // lock_pointer
                    &raw const LOCKED_POINTER_INTERFACE,
                    null_mut::<c_void>(),
                    surface,
                    pointer,
                    null_mut::<c_void>(),
                    LIFETIME_PERSISTENT,
                );
                (api.flush)(display);
                Some(Self {
                    api,
                    display,
                    locked,
                    relative,
                    _sender: sender,
                })
            }
        }
    }

    impl Drop for Lock {
        fn drop(&mut self) {
            // SAFETY: Both objects are live, destroying them stops events using the sender.
            unsafe {
                for proxy in [self.locked, self.relative] {
                    (self.api.marshal)(proxy, 0); // destroy
                    (self.api.destroy)(proxy);
                }
                (self.api.flush)(self.display);
            }
        }
    }
}
