/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Native window layers from Windows 8.1: a DirectComposition visual for each layer under a
//! root visual of the window, which the desktop compositor blends below child windows.
//! Renderers present composition swap chains as the content of their visual. Without
//! DirectComposition, like on Windows 7, the layers share the client area and renderers
//! composite them themselves. The target clips the visuals to the client area, and hidden
//! layers leave the visual tree instead of being clipped away. The vtables follow `dcomp.h`
//! with MSVC overload ordering. The value-based offset setters follow their animation overloads.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::rc::{Rc, Weak};

use super::com::ComPtr;
use super::headers::{GUID, HRESULT, HWND, IUnknownVtbl};
use super::window_dpi;
use crate::{LogicalRect, NativeContentHandle};

const IID_IDCOMPOSITION_DESKTOP_DEVICE: GUID = GUID {
    data1: 0x5f46_33fe,
    data2: 0x1e08,
    data3: 0x4cb8,
    data4: [0x8c, 0x75, 0xce, 0x24, 0x33, 0x3f, 0x56, 0x02],
};

#[repr(C)]
struct IDCompositionDesktopDevice {
    vtbl: *const IDCompositionDesktopDeviceVtbl,
}

#[repr(C)]
struct IDCompositionDevice2Vtbl {
    base: IUnknownVtbl,
    commit: unsafe extern "system" fn(*mut IDCompositionDesktopDevice) -> HRESULT,
    _unused_4_5: [*const c_void; 2],
    create_visual: unsafe extern "system" fn(
        *mut IDCompositionDesktopDevice,
        *mut *mut IDCompositionVisual,
    ) -> HRESULT,
    _unused_7_23: [*const c_void; 17],
}

#[repr(C)]
struct IDCompositionDesktopDeviceVtbl {
    base: IDCompositionDevice2Vtbl,
    create_target_for_hwnd: unsafe extern "system" fn(
        *mut IDCompositionDesktopDevice,
        HWND,
        i32,
        *mut *mut IDCompositionTarget,
    ) -> HRESULT,
}

#[repr(C)]
struct IDCompositionTarget {
    vtbl: *const IDCompositionTargetVtbl,
}

#[repr(C)]
struct IDCompositionTargetVtbl {
    base: IUnknownVtbl,
    set_root:
        unsafe extern "system" fn(*mut IDCompositionTarget, *mut IDCompositionVisual) -> HRESULT,
}

#[repr(C)]
struct IDCompositionVisual {
    vtbl: *const IDCompositionVisualVtbl,
}

#[repr(C)]
struct IDCompositionVisualVtbl {
    base: IUnknownVtbl,
    _unused_3: *const c_void,
    set_offset_x: unsafe extern "system" fn(*mut IDCompositionVisual, f32) -> HRESULT,
    _unused_5: *const c_void,
    set_offset_y: unsafe extern "system" fn(*mut IDCompositionVisual, f32) -> HRESULT,
    _unused_7_15: [*const c_void; 9],
    add_visual: unsafe extern "system" fn(
        *mut IDCompositionVisual,
        *mut IDCompositionVisual,
        i32,
        *mut IDCompositionVisual,
    ) -> HRESULT,
    remove_visual:
        unsafe extern "system" fn(*mut IDCompositionVisual, *mut IDCompositionVisual) -> HRESULT,
}

struct CompositionDevice(ComPtr<IDCompositionDesktopDevice>);

impl CompositionDevice {
    fn commit(&self) {
        unsafe { ((*self.0.vtbl).base.commit)(self.0.as_ptr()) };
    }

    fn create_visual(&self) -> Result<ComPtr<IDCompositionVisual>, HRESULT> {
        unsafe { ComPtr::create(|out| ((*self.0.vtbl).base.create_visual)(self.0.as_ptr(), out)) }
    }
}

impl Drop for CompositionDevice {
    fn drop(&mut self) {
        self.commit();
    }
}

// The composition device of a window with its target and root visual, and the layer
// visuals from bottom to top with whether they are in the visual tree.
struct Composition {
    root: ComPtr<IDCompositionVisual>,
    _target: ComPtr<IDCompositionTarget>,
    // Commit after releasing the root and target.
    device: CompositionDevice,
    order: RefCell<Vec<(*mut IDCompositionVisual, bool)>>,
}

impl Composition {
    // Put a layer visual in or out of the visual tree, above the nearest lower one in it.
    unsafe fn show(&self, visual: *mut IDCompositionVisual, shown: bool) {
        let mut order = self.order.borrow_mut();
        let Some(index) = order.iter().position(|(other, _)| *other == visual) else {
            return;
        };
        if std::mem::replace(&mut order[index].1, shown) == shown {
            return;
        }
        unsafe {
            if shown {
                // Without a lower visual in the tree, it goes to the bottom.
                let below = order[..index]
                    .iter()
                    .rev()
                    .find(|(_, shown)| *shown)
                    .map_or(null_mut(), |(below, _)| *below);
                ((*self.root.vtbl).add_visual)(self.root.as_ptr(), visual, 1, below);
            } else {
                ((*self.root.vtbl).remove_visual)(self.root.as_ptr(), visual);
            }
            self.device.commit();
        }
    }
}

struct LayerState {
    _visual: ComPtr<IDCompositionVisual>,
    composition: Rc<Composition>,
    hwnd: HWND,
}

thread_local! {
    static COMPOSITIONS: RefCell<HashMap<usize, Weak<Composition>>> = RefCell::new(HashMap::new());
    static LAYERS: RefCell<HashMap<usize, LayerState>> = RefCell::new(HashMap::new());
}

// The composition of a window, created once, none without DirectComposition.
fn composition(hwnd: HWND) -> Option<Rc<Composition>> {
    let existing = COMPOSITIONS
        .with_borrow(|compositions| compositions.get(&(hwnd as usize)).and_then(Weak::upgrade));
    if existing.is_some() {
        return existing;
    }
    let create_device = optional_proc!(
        10,
        "dcomp",
        DCompositionCreateDevice2(*mut c_void, *const GUID, *mut *mut c_void) -> i32
    )?;
    unsafe {
        let device = ComPtr::create(|out: *mut *mut IDCompositionDesktopDevice| {
            create_device(null_mut(), &IID_IDCOMPOSITION_DESKTOP_DEVICE, out.cast())
        })
        .ok()?;
        // Below child windows like a web view.
        let target = ComPtr::create(|out| {
            ((*device.vtbl).create_target_for_hwnd)(device.as_ptr(), hwnd, 0, out)
        })
        .ok()?;
        let root =
            ComPtr::create(|out| ((*device.vtbl).base.create_visual)(device.as_ptr(), out)).ok()?;
        ((*target.vtbl).set_root)(target.as_ptr(), root.as_ptr());
        let device = CompositionDevice(device);
        device.commit();
        let composition = Rc::new(Composition {
            root,
            _target: target,
            device,
            order: RefCell::new(Vec::new()),
        });
        COMPOSITIONS.with_borrow_mut(|compositions| {
            compositions.retain(|_, composition| composition.strong_count() > 0);
            compositions.insert(hwnd as usize, Rc::downgrade(&composition));
        });
        Some(composition)
    }
}

fn hwnd(content: NativeContentHandle) -> HWND {
    let NativeContentHandle::Win32(hwnd) = content else {
        panic!("expected a Win32 window");
    };
    hwnd
}

fn visual(layer: NativeContentHandle) -> *mut IDCompositionVisual {
    let NativeContentHandle::DirectComposition { visual, .. } = layer else {
        panic!("expected a DirectComposition layer");
    };
    visual.cast()
}

/// Whether layers get native visuals, which needs DirectComposition.
pub(crate) unsafe fn native_layers(content: NativeContentHandle) -> bool {
    matches!(content, NativeContentHandle::Win32(_)) && composition(hwnd(content)).is_some()
}

/// Add a visual above or below a neighboring layer visual, or else above every other one.
pub(crate) unsafe fn add_layer_view(
    content: NativeContentHandle,
    neighbor: Option<(NativeContentHandle, bool)>,
) -> NativeContentHandle {
    let hwnd = hwnd(content);
    let composition = composition(hwnd).expect("DirectComposition is available");
    let owned = composition
        .device
        .create_visual()
        .expect("cannot create a layer visual");
    let visual = owned.as_ptr();
    {
        let mut order = composition.order.borrow_mut();
        let index = neighbor
            .and_then(|(neighbor, above)| {
                let neighbor = self::visual(neighbor);
                let index = order.iter().position(|(other, _)| *other == neighbor)?;
                Some(index + usize::from(above))
            })
            .unwrap_or(order.len());
        order.insert(index, (visual, false));
    }
    unsafe { composition.show(visual, true) };
    let handle = NativeContentHandle::DirectComposition {
        device: composition.device.0.as_ptr().cast(),
        visual: visual.cast(),
    };
    LAYERS.with_borrow_mut(|layers| {
        layers.insert(
            visual as usize,
            LayerState {
                _visual: owned,
                composition,
                hwnd,
            },
        );
    });
    handle
}

/// Place a visual at an area of the client area, or at its origin when it fills it.
pub(crate) unsafe fn set_layer_view_frame(
    _: NativeContentHandle,
    layer: NativeContentHandle,
    frame: Option<LogicalRect>,
) {
    let visual = visual(layer);
    LAYERS.with_borrow(|layers| {
        let Some(state) = layers.get(&(visual as usize)) else {
            return;
        };
        unsafe {
            let scale = window_dpi(state.hwnd) as f32 / 96.0;
            let (x, y) = frame.map_or((0.0, 0.0), |frame| (frame.x * scale, frame.y * scale));
            let vtbl = &*(*visual).vtbl;
            (vtbl.set_offset_x)(visual, x.round());
            (vtbl.set_offset_y)(visual, y.round());
            state.composition.device.commit();
        }
    });
}

/// Take a hidden visual out of the visual tree, the cheapest way to hide it.
pub(crate) unsafe fn set_layer_view_hidden(layer: NativeContentHandle, hidden: bool) {
    let visual = visual(layer);
    LAYERS.with_borrow(|layers| {
        if let Some(state) = layers.get(&(visual as usize)) {
            unsafe { state.composition.show(visual, !hidden) };
        }
    });
}

/// Remove a visual from the window and release it.
pub(crate) unsafe fn remove_layer_view(layer: NativeContentHandle) {
    let visual = visual(layer);
    let Some(state) = LAYERS.with_borrow_mut(|layers| layers.remove(&(visual as usize))) else {
        return;
    };
    unsafe {
        state.composition.show(visual, false);
        state
            .composition
            .order
            .borrow_mut()
            .retain(|(other, _)| *other != visual);
    }
}
