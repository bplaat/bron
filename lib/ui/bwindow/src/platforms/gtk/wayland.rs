/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The `zwp_pointer_constraints_v1`, `zwp_relative_pointer_manager_v1` and
//! `wp_viewporter` protocols, with the interface definitions their generated C code
//! would have. Like everything
//! else the GTK backend uses, libwayland-client is GDK's, so its functions and the GDK
//! Wayland backend functions are looked up in the process instead of being linked.

use std::cell::{Cell, OnceCell, UnsafeCell};
use std::ffi::{CStr, c_char, c_void};
use std::mem::transmute;
use std::ptr::{null, null_mut};

use super::headers::GdkDisplay;
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

// `wp_viewport` and `wl_surface`.
static VIEWPORTER_TYPES: Types<2> = Types::new([&raw const VIEWPORT_INTERFACE, null()]);
static VIEWPORTER_METHODS: [Message; 2] = [
    message(c"destroy", c"", NO_TYPES.as_ptr()),
    message(c"get_viewport", c"no", VIEWPORTER_TYPES.as_ptr()),
];
static VIEWPORTER_INTERFACE: Interface = Interface {
    name: c"wp_viewporter".as_ptr(),
    version: 1,
    method_count: 2,
    methods: VIEWPORTER_METHODS.as_ptr(),
    event_count: 0,
    events: null(),
};

static VIEWPORT_METHODS: [Message; 3] = [
    message(c"destroy", c"", NO_TYPES.as_ptr()),
    message(c"set_source", c"ffff", NO_TYPES.as_ptr()),
    message(c"set_destination", c"ii", NO_TYPES.as_ptr()),
];
static VIEWPORT_INTERFACE: Interface = Interface {
    name: c"wp_viewport".as_ptr(),
    version: 1,
    method_count: 3,
    methods: VIEWPORT_METHODS.as_ptr(),
    event_count: 0,
    events: null(),
};

type Marshal = unsafe extern "C" fn(*mut c_void, u32, ...);
type GetVersion = unsafe extern "C" fn(*mut c_void) -> u32;
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
    get_version: GetVersion,
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
            get_version: transmute::<*mut c_void, GetVersion>(symbol(c"wl_proxy_get_version")?),
            constructor: transmute::<*mut c_void, Constructor>(symbol(
                c"wl_proxy_marshal_constructor",
            )?),
            constructor_versioned: transmute::<*mut c_void, ConstructorVersioned>(symbol(
                c"wl_proxy_marshal_constructor_versioned",
            )?),
            add_listener: transmute::<*mut c_void, AddListener>(symbol(c"wl_proxy_add_listener")?),
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
        (*VIEWPORTER_TYPES.0.get())[1] = types[1];
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
    viewporter: *mut c_void,
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
        } else if interface == c"wp_viewporter" {
            (&mut globals.viewporter, &raw const VIEWPORTER_INTERFACE)
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

// Binds the protocol globals of the display once, the missing ones are null.
unsafe fn globals(api: Api, display: *mut c_void) -> Globals {
    if let Some(globals) = GLOBALS.get() {
        return globals;
    }
    let mut globals = Globals {
        constraints: null_mut(),
        relative: null_mut(),
        viewporter: null_mut(),
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
    GLOBALS.set(Some(globals));
    globals
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
            let globals = globals(api, display);
            if globals.constraints.is_null() || globals.relative.is_null() {
                return None;
            }
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

/// The Wayland surface of a mapped GDK window.
pub(in crate::platforms::gtk) unsafe fn wayland_surface(window: *mut c_void) -> *mut c_void {
    // SAFETY: The caller supplies a live window.
    api().map_or(null_mut(), |api| unsafe { (api.wl_surface)(window) })
}

/// A viewport of the Wayland surface of a GDK window, which crops and scales what it
/// shows of its buffer.
pub(in crate::platforms::gtk) struct Viewport {
    api: Api,
    display: *mut c_void,
    surface: *mut c_void,
    viewport: *mut c_void,
}

impl Viewport {
    /// The viewport of a mapped window, none without the protocol.
    pub(in crate::platforms::gtk) unsafe fn new(
        display: *mut GdkDisplay,
        window: *mut c_void,
    ) -> Option<Self> {
        let api = api()?;
        // SAFETY: The caller supplies a live Wayland display and mapped window.
        unsafe {
            let display = (api.wl_display)(display);
            let surface = (api.wl_surface)(window);
            let globals = globals(api, display);
            if surface.is_null() || globals.viewporter.is_null() {
                return None;
            }
            let viewport = (api.constructor)(
                globals.viewporter,
                1, // get_viewport
                &raw const VIEWPORT_INTERFACE,
                null_mut::<c_void>(),
                surface,
            );
            Some(Self {
                api,
                display,
                surface,
                viewport,
            })
        }
    }

    /// The Wayland surface the viewport belongs to, which GDK replaces when it maps the
    /// window again.
    pub(in crate::platforms::gtk) const fn surface(&self) -> *mut c_void {
        self.surface
    }

    /// Show the logical source rectangle of the buffer at a logical size, applied at
    /// once.
    pub(in crate::platforms::gtk) fn set(&self, source: [f64; 4], width: i32, height: i32) {
        let fixed = |value: f64| (value * 256.0).round() as i32;
        // SAFETY: The viewport and its surface are live.
        unsafe {
            (self.api.marshal)(
                self.viewport,
                1, // set_source
                fixed(source[0]),
                fixed(source[1]),
                fixed(source[2]),
                fixed(source[3]),
            );
            (self.api.marshal)(self.viewport, 2, width, height); // set_destination
            (self.api.marshal)(self.surface, 6); // wl_surface.commit
            (self.api.flush)(self.display);
        }
    }
}

impl Drop for Viewport {
    fn drop(&mut self) {
        // SAFETY: The viewport is live.
        unsafe {
            (self.api.marshal)(self.viewport, 0); // destroy
            (self.api.destroy)(self.viewport);
        }
    }
}

/// Set the buffer scale of a Wayland surface whose buffers GDK does not attach itself, like
/// the Vulkan swapchain of a GPU surface, which `wl_surface.set_buffer_scale` needs since
/// version 3.
pub(crate) unsafe fn set_buffer_scale(surface: *mut c_void, scale: i32) {
    let Some(api) = api() else {
        return;
    };
    unsafe {
        if (api.get_version)(surface) >= 3 {
            (api.marshal)(surface, 8, scale);
        }
    }
}
