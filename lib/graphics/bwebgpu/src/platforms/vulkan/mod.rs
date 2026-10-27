/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Vulkan 1.1, one graphics/present queue, with explicit uploads and synchronization.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::ptr;
use std::rc::Rc;

#[cfg(all(feature = "window", target_os = "android"))]
pub(crate) use android::Surface;
#[cfg(all(feature = "window", not(target_os = "android")))]
pub(crate) use gtk::Surface;
#[cfg(feature = "window")]
pub(crate) use surface::Factory;
#[cfg(feature = "window")]
use surface::Swapchain;

use super::*;
use crate::platform::headers as vk;
use crate::platform::headers::{GetProc, dlclose, dlopen, dlsym};

// Defined before the modules so its textual macro scope covers them.
macro_rules! call {
    ($instance:expr, $name:ident $(, $arg:expr)* $(,)?) => {{
        let function: vk::$name = std::mem::transmute($instance.function(CStr::from_bytes_with_nul_unchecked(concat!("vk", stringify!($name), "\0").as_bytes())));
        function($($arg),*)
    }};
}

#[cfg(all(feature = "window", target_os = "android"))]
mod android;
#[cfg(all(feature = "window", not(target_os = "android")))]
mod gtk;
mod headers;
#[cfg(feature = "window")]
mod surface;
#[cfg(all(feature = "window", not(target_os = "android")))]
mod widget;

pub(super) struct Instance {
    library: *mut c_void,
    handle: vk::Instance,
    get: GetProc,
    functions: RefCell<HashMap<&'static CStr, *const c_void>>,
}

impl Instance {
    fn function(&self, name: &'static CStr) -> *const c_void {
        *self.functions.borrow_mut().entry(name).or_insert_with(|| {
            let function = unsafe { (self.get)(self.handle, name.as_ptr()) };
            assert!(!function.is_null(), "Missing Vulkan function {name:?}");
            function
        })
    }
}

fn check(result: i32) -> Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(Error(format!("Vulkan error {result}")))
    }
}

const fn format(value: TextureFormat) -> u32 {
    match value {
        TextureFormat::Rgba8Unorm => 37,
        TextureFormat::Rgba8UnormSrgb => 43,
        TextureFormat::Bgra8Unorm => 44,
        TextureFormat::Bgra8UnormSrgb => 50,
        TextureFormat::Depth32Float => 126,
    }
}

impl Instance {
    fn new(platform: Option<&CStr>) -> Result<Rc<Self>> {
        unsafe {
            // OpenBSD has no `.so.1` name; its loader resolves the unversioned one.
            let library = [c"libvulkan.so.1", c"libvulkan.so"]
                .iter()
                .map(|name| dlopen(name.as_ptr(), 2))
                .find(|library| !library.is_null())
                .ok_or_else(|| Error("Cannot load the Vulkan loader".into()))?;
            let get = dlsym(library, c"vkGetInstanceProcAddr".as_ptr());
            if get.is_null() {
                dlclose(library);
                return Err(Error("Missing vkGetInstanceProcAddr".into()));
            }
            let get: GetProc = std::mem::transmute(get);
            let create: vk::CreateInstance =
                std::mem::transmute(get(ptr::null_mut(), c"vkCreateInstance".as_ptr()));
            let extensions = platform.map_or(Vec::new(), |platform| {
                vec![c"VK_KHR_surface".as_ptr(), platform.as_ptr()]
            });
            let application = vk::ApplicationInfo {
                s_type: 0,
                api_version: (1 << 22) | (1 << 12),
                ..Default::default()
            };
            let info = vk::InstanceCreateInfo {
                s_type: 1,
                p_application_info: &application,
                enabled_extension_count: extensions.len() as u32,
                pp_enabled_extension_names: extensions.as_ptr(),
                ..Default::default()
            };
            let mut handle = ptr::null_mut();
            if let Err(error) = check(create(&info, ptr::null(), &mut handle)) {
                dlclose(library);
                return Err(error);
            }
            Ok(Rc::new(Self {
                library,
                handle,
                get,
                functions: RefCell::new(HashMap::new()),
            }))
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            call!(self, DestroyInstance, self.handle, ptr::null());
            dlclose(self.library);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RenderPassKey {
    format: u32,
    clear_color: bool,
    has_depth: bool,
    clear_depth: bool,
    store_depth: bool,
}

struct NativeDevice {
    instance: Rc<Instance>,
    handle: vk::Device,
    #[cfg(feature = "window")]
    physical: vk::PhysicalDevice,
    queue: vk::Queue,
    family: u32,
    memory: vk::PhysicalDeviceMemoryProperties,
    pools: RefCell<Vec<Weak<Memory>>>,
    render_passes: RefCell<HashMap<RenderPassKey, u64>>,
    // Binding roles are fixed, so every pipeline and bind group shares one layout.
    set_layout: u64,
    pipeline_layout: u64,
}

impl Drop for NativeDevice {
    fn drop(&mut self) {
        unsafe {
            call!(self.instance, DeviceWaitIdle, self.handle);
            for (_, render) in self.render_passes.get_mut().drain() {
                call!(
                    self.instance,
                    DestroyRenderPass,
                    self.handle,
                    render,
                    ptr::null()
                );
            }
            call!(
                self.instance,
                DestroyPipelineLayout,
                self.handle,
                self.pipeline_layout,
                ptr::null()
            );
            call!(
                self.instance,
                DestroyDescriptorSetLayout,
                self.handle,
                self.set_layout,
                ptr::null()
            );
            call!(self.instance, DestroyDevice, self.handle, ptr::null());
        }
    }
}

struct Memory {
    device: Rc<NativeDevice>,
    handle: u64,
    size: u64,
    kind: u32,
    free: RefCell<allocation::Ranges>,
}

impl Drop for Memory {
    fn drop(&mut self) {
        unsafe {
            call!(
                self.device.instance,
                FreeMemory,
                self.device.handle,
                self.handle,
                ptr::null()
            );
        }
    }
}

impl Memory {
    fn reserve(&self, size: u64, alignment: u64) -> Option<u64> {
        self.free.borrow_mut().allocate(size, alignment)
    }

    fn release(&self, range: Range<u64>) {
        self.free.borrow_mut().release(range);
    }
}

impl NativeDevice {
    fn allocate(
        self: &Rc<Self>,
        requirements: vk::MemoryRequirements,
        flags: u32,
        pooled: bool,
    ) -> Result<(Rc<Memory>, u64)> {
        let kind = (0..self.memory.memory_type_count)
            .find(|&i| {
                requirements.memory_type_bits & (1 << i) != 0
                    && self.memory.memory_types[i as usize].property_flags & flags == flags
            })
            .ok_or_else(|| Error("No compatible Vulkan memory type".into()))?;
        if pooled {
            self.pools
                .borrow_mut()
                .retain(|pool| pool.strong_count() > 0);
            for memory in self.pools.borrow().iter().filter_map(Weak::upgrade) {
                if memory.kind == kind
                    && let Some(offset) = memory.reserve(requirements.size, requirements.alignment)
                {
                    return Ok((memory, offset));
                }
            }
        }
        let size = if pooled {
            requirements.size.max(64 * 1024 * 1024)
        } else {
            requirements.size
        };
        let mut handle = 0;
        let info = vk::MemoryAllocateInfo {
            s_type: 5,
            allocation_size: size,
            memory_type_index: kind,
            ..Default::default()
        };
        unsafe {
            check(call!(
                self.instance,
                AllocateMemory,
                self.handle,
                &info,
                ptr::null(),
                &mut handle
            ))?;
        }
        let memory = Rc::new(Memory {
            device: self.clone(),
            handle,
            size,
            kind,
            free: RefCell::new(allocation::Ranges::new(size)),
        });
        let offset = memory
            .reserve(requirements.size, requirements.alignment)
            .expect("fresh Vulkan allocation is large enough");
        if pooled {
            self.pools.borrow_mut().push(Rc::downgrade(&memory));
        }
        Ok((memory, offset))
    }

    fn create_layouts(&mut self) -> Result<()> {
        // Uniform 0, texture 1, sampler 2 and read-only storage 3, see create_bind_group_layout.
        let bindings = [(6, 1 | 16), (2, 16), (0, 16), (7, 1)]
            .into_iter()
            .enumerate()
            .map(
                |(binding, (descriptor_type, stage_flags))| vk::DescriptorSetLayoutBinding {
                    binding: binding as u32,
                    descriptor_type,
                    descriptor_count: 1,
                    stage_flags,
                    ..Default::default()
                },
            )
            .collect::<Vec<_>>();
        unsafe {
            let info = vk::DescriptorSetLayoutCreateInfo {
                s_type: 32,
                binding_count: bindings.len() as u32,
                p_bindings: bindings.as_ptr(),
                ..Default::default()
            };
            check(call!(
                self.instance,
                CreateDescriptorSetLayout,
                self.handle,
                &info,
                ptr::null(),
                &mut self.set_layout
            ))?;
            let info = vk::PipelineLayoutCreateInfo {
                s_type: 30,
                set_layout_count: 1,
                p_set_layouts: &self.set_layout,
                ..Default::default()
            };
            check(call!(
                self.instance,
                CreatePipelineLayout,
                self.handle,
                &info,
                ptr::null(),
                &mut self.pipeline_layout
            ))
        }
    }

    fn buffer(self: &Rc<Self>, size: u64, staging: bool) -> Result<Buffer> {
        unsafe {
            let info = vk::BufferCreateInfo {
                s_type: 12,
                size,
                usage: if staging {
                    1 | 2
                } else {
                    2 | 16 | 32 | 64 | 128
                },
                ..Default::default()
            };
            let mut handle = 0;
            check(call!(
                self.instance,
                CreateBuffer,
                self.handle,
                &info,
                ptr::null(),
                &mut handle
            ))?;
            let mut requirements = vk::MemoryRequirements::default();
            call!(
                self.instance,
                GetBufferMemoryRequirements,
                self.handle,
                handle,
                &mut requirements
            );
            let (memory, offset) =
                match self.allocate(requirements, if staging { 2 | 4 } else { 1 }, !staging) {
                    Ok(value) => value,
                    Err(e) => {
                        call!(
                            self.instance,
                            DestroyBuffer,
                            self.handle,
                            handle,
                            ptr::null()
                        );
                        return Err(e);
                    }
                };
            let mut result = Buffer {
                handle,
                memory,
                offset,
                allocation_size: requirements.size,
                capacity: size,
                mapped: ptr::null_mut(),
            };
            check(call!(
                self.instance,
                BindBufferMemory,
                self.handle,
                handle,
                result.memory.handle,
                offset
            ))?;
            if staging {
                let mut mapped = ptr::null_mut();
                check(call!(
                    self.instance,
                    MapMemory,
                    self.handle,
                    result.memory.handle,
                    0,
                    result.memory.size,
                    0,
                    &mut mapped
                ))?;
                result.mapped = mapped.cast();
            }
            Ok(result)
        }
    }
}

pub(super) struct Buffer {
    handle: u64,
    memory: Rc<Memory>,
    offset: u64,
    allocation_size: u64,
    capacity: u64,
    // Staging buffers own coherent memory that stays mapped until it is freed.
    mapped: *mut u8,
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            let device = &self.memory.device;
            call!(
                device.instance,
                DestroyBuffer,
                device.handle,
                self.handle,
                ptr::null()
            );
        }
        self.memory
            .release(self.offset..self.offset + self.allocation_size);
    }
}

pub(super) struct Texture {
    image: u64,
    view: u64,
    memory: Option<Rc<Memory>>,
    #[cfg(feature = "window")]
    swap: Option<Rc<Swapchain>>,
    device: Rc<NativeDevice>,
    layout: Cell<u32>,
    mips: u32,
    layers: u32,
    depth: bool,
    #[cfg(feature = "window")]
    index: usize,
}

impl Drop for Texture {
    fn drop(&mut self) {
        if self.memory.is_some() {
            unsafe {
                call!(
                    self.device.instance,
                    DestroyImageView,
                    self.device.handle,
                    self.view,
                    ptr::null()
                );
                call!(
                    self.device.instance,
                    DestroyImage,
                    self.device.handle,
                    self.image,
                    ptr::null()
                );
            }
        }
    }
}

pub(super) struct Sampler {
    handle: u64,
    device: Rc<NativeDevice>,
}

impl Drop for Sampler {
    fn drop(&mut self) {
        unsafe {
            call!(
                self.device.instance,
                DestroySampler,
                self.device.handle,
                self.handle,
                ptr::null()
            );
        }
    }
}

pub(super) struct Pipeline {
    handle: u64,
    render: u64,
    device: Rc<NativeDevice>,
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        unsafe {
            let d = &self.device;
            call!(
                d.instance,
                DestroyPipeline,
                d.handle,
                self.handle,
                ptr::null()
            );
        }
    }
}

struct Frame {
    device: Rc<NativeDevice>,
    pool: u64,
    command: vk::CommandBuffer,
    fence: u64,
    available: u64,
    descriptors: u64,
    busy: bool,
    uploads: Vec<Upload>,
    passes: Vec<Pass>,
    staging: Vec<Buffer>,
    framebuffers: Vec<u64>,
}

pub(super) struct Context {
    device: Rc<NativeDevice>,
    frames: RefCell<Vec<Frame>>,
    slot: Cell<usize>,
    #[cfg(feature = "window")]
    acquired: RefCell<Option<(Rc<Swapchain>, usize)>>,
    #[cfg(feature = "window")]
    acquisition_waited: Cell<bool>,
    #[cfg(feature = "window")]
    surface_status: Cell<i32>,
}

const fn blend_factor(value: BlendFactor) -> u32 {
    match value {
        BlendFactor::Zero => 0,
        BlendFactor::One => 1,
        BlendFactor::Src => 2,
        BlendFactor::OneMinusSrc => 3,
        BlendFactor::SrcAlpha => 6,
        BlendFactor::OneMinusSrcAlpha => 7,
        BlendFactor::Dst => 4,
        BlendFactor::OneMinusDst => 5,
        BlendFactor::DstAlpha => 8,
        BlendFactor::OneMinusDstAlpha => 9,
        BlendFactor::SrcAlphaSaturated => 10,
    }
}

impl Context {
    pub(super) fn new() -> Result<Rc<Self>> {
        Self::create(&Instance::new(None)?, false, None)
    }

    /// Create a device on the first graphics queue, which must also present to `surface`.
    fn create(instance: &Rc<Instance>, swapchain: bool, surface: Option<u64>) -> Result<Rc<Self>> {
        unsafe {
            let mut count = 0;
            check(call!(
                instance,
                EnumeratePhysicalDevices,
                instance.handle,
                &mut count,
                ptr::null_mut()
            ))?;
            let mut physicals = vec![ptr::null_mut(); count as usize];
            check(call!(
                instance,
                EnumeratePhysicalDevices,
                instance.handle,
                &mut count,
                physicals.as_mut_ptr()
            ))?;
            let mut selected = None;
            for physical in physicals {
                let mut count = 0;
                call!(
                    instance,
                    GetPhysicalDeviceQueueFamilyProperties,
                    physical,
                    &mut count,
                    ptr::null_mut()
                );
                let mut families = vec![vk::QueueFamilyProperties::default(); count as usize];
                call!(
                    instance,
                    GetPhysicalDeviceQueueFamilyProperties,
                    physical,
                    &mut count,
                    families.as_mut_ptr()
                );
                for (family, properties) in families.iter().enumerate() {
                    let mut supported = 1;
                    if let Some(surface) = surface {
                        check(call!(
                            instance,
                            GetPhysicalDeviceSurfaceSupportKHR,
                            physical,
                            family as u32,
                            surface,
                            &mut supported
                        ))?;
                    }
                    if properties.queue_flags & 1 != 0 && supported != 0 {
                        selected = Some((physical, family as u32));
                        break;
                    }
                }
                if selected.is_some() {
                    break;
                }
            }
            let (physical, family) =
                selected.ok_or_else(|| Error("No suitable Vulkan graphics queue".into()))?;
            let priority = 1f32;
            let queue_info = vk::DeviceQueueCreateInfo {
                s_type: 2,
                queue_family_index: family,
                queue_count: 1,
                p_queue_priorities: &priority,
                ..Default::default()
            };
            let extensions = [c"VK_KHR_swapchain".as_ptr()];
            let info = vk::DeviceCreateInfo {
                s_type: 3,
                queue_create_info_count: 1,
                p_queue_create_infos: &queue_info,
                enabled_extension_count: u32::from(swapchain),
                pp_enabled_extension_names: extensions.as_ptr(),
                ..Default::default()
            };
            let mut handle = ptr::null_mut();
            check(call!(
                instance,
                CreateDevice,
                physical,
                &info,
                ptr::null(),
                &mut handle
            ))?;
            let mut queue = ptr::null_mut();
            call!(instance, GetDeviceQueue, handle, family, 0, &mut queue);
            let mut memory = vk::PhysicalDeviceMemoryProperties::default();
            call!(
                instance,
                GetPhysicalDeviceMemoryProperties,
                physical,
                &mut memory
            );
            // Dropping the device on error also destroys any layout created so far.
            let mut device = NativeDevice {
                instance: instance.clone(),
                handle,
                #[cfg(feature = "window")]
                physical,
                queue,
                family,
                memory,
                pools: RefCell::new(Vec::new()),
                render_passes: RefCell::new(HashMap::new()),
                set_layout: 0,
                pipeline_layout: 0,
            };
            device.create_layouts()?;
            let device = Rc::new(device);
            let context = Rc::new(Self {
                device,
                frames: RefCell::new(Vec::new()),
                slot: Cell::new(0),
                #[cfg(feature = "window")]
                acquired: RefCell::new(None),
                #[cfg(feature = "window")]
                acquisition_waited: Cell::new(false),
                #[cfg(feature = "window")]
                surface_status: Cell::new(0),
            });
            for _ in 0..2 {
                context.frames.borrow_mut().push(context.frame()?);
            }
            Ok(context)
        }
    }

    fn frame(&self) -> Result<Frame> {
        unsafe {
            let d = &self.device;
            let mut frame = Frame {
                device: d.clone(),
                pool: 0,
                command: ptr::null_mut(),
                fence: 0,
                available: 0,
                descriptors: 0,
                busy: false,
                uploads: Vec::new(),
                passes: Vec::new(),
                staging: Vec::new(),
                framebuffers: Vec::new(),
            };
            let info = vk::CommandPoolCreateInfo {
                s_type: 39,
                queue_family_index: d.family,
                ..Default::default()
            };
            check(call!(
                d.instance,
                CreateCommandPool,
                d.handle,
                &info,
                ptr::null(),
                &mut frame.pool
            ))?;
            let info = vk::CommandBufferAllocateInfo {
                s_type: 40,
                command_pool: frame.pool,
                command_buffer_count: 1,
                ..Default::default()
            };
            check(call!(
                d.instance,
                AllocateCommandBuffers,
                d.handle,
                &info,
                &mut frame.command
            ))?;
            let info = vk::FenceCreateInfo {
                s_type: 8,
                ..Default::default()
            };
            check(call!(
                d.instance,
                CreateFence,
                d.handle,
                &info,
                ptr::null(),
                &mut frame.fence
            ))?;
            let info = vk::SemaphoreCreateInfo {
                s_type: 9,
                ..Default::default()
            };
            check(call!(
                d.instance,
                CreateSemaphore,
                d.handle,
                &info,
                ptr::null(),
                &mut frame.available
            ))?;
            let sizes = [
                vk::DescriptorPoolSize {
                    ty: 6,
                    descriptor_count: 256,
                },
                vk::DescriptorPoolSize {
                    ty: 7,
                    descriptor_count: 256,
                },
                vk::DescriptorPoolSize {
                    ty: 2,
                    descriptor_count: 256,
                },
                vk::DescriptorPoolSize {
                    ty: 0,
                    descriptor_count: 256,
                },
            ];
            let info = vk::DescriptorPoolCreateInfo {
                s_type: 33,
                max_sets: 256,
                pool_size_count: sizes.len() as u32,
                p_pool_sizes: sizes.as_ptr(),
                ..Default::default()
            };
            check(call!(
                d.instance,
                CreateDescriptorPool,
                d.handle,
                &info,
                ptr::null(),
                &mut frame.descriptors
            ))?;
            Ok(frame)
        }
    }

    fn wait(&self, frame: &mut Frame, timeout: u64) -> Result<bool> {
        unsafe {
            let d = &self.device;
            if frame.busy {
                let result = call!(
                    d.instance,
                    WaitForFences,
                    d.handle,
                    1,
                    &frame.fence,
                    1,
                    timeout
                );
                if result == 2 {
                    return Ok(false);
                }
                check(result)?;
                frame.busy = false;
            }
            for framebuffer in frame.framebuffers.drain(..) {
                call!(
                    d.instance,
                    DestroyFramebuffer,
                    d.handle,
                    framebuffer,
                    ptr::null()
                );
            }
            frame.passes.clear();
            frame.uploads.clear();
            check(call!(d.instance, ResetCommandPool, d.handle, frame.pool, 0))?;
            check(call!(
                d.instance,
                ResetDescriptorPool,
                d.handle,
                frame.descriptors,
                0
            ))?;
            Ok(true)
        }
    }

    pub(super) fn buffer(&self, size: u64, _: BufferUsages) -> Result<Buffer> {
        self.device.buffer(size, false)
    }

    fn view(&self, image: u64, fmt: TextureFormat, mips: u32, layers: u32) -> Result<u64> {
        let info = vk::ImageViewCreateInfo {
            s_type: 15,
            image,
            view_type: if layers > 1 { 5 } else { 1 },
            format: format(fmt),
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: if fmt == TextureFormat::Depth32Float {
                    2
                } else {
                    1
                },
                level_count: mips,
                layer_count: layers,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut view = 0;
        unsafe {
            check(call!(
                self.device.instance,
                CreateImageView,
                self.device.handle,
                &info,
                ptr::null(),
                &mut view
            ))?;
        }
        Ok(view)
    }

    pub(super) fn texture(&self, desc: &TextureDescriptor<'_>) -> Result<Texture> {
        unsafe {
            let d = &self.device;
            let depth = desc.format == TextureFormat::Depth32Float;
            let usage = if desc.usage.contains(TextureUsages::COPY_DST) {
                2
            } else {
                0
            } | if desc.usage.contains(TextureUsages::COPY_SRC) {
                1
            } else {
                0
            } | if desc.usage.contains(TextureUsages::TEXTURE_BINDING) {
                4
            } else {
                0
            } | if desc.usage.contains(TextureUsages::RENDER_ATTACHMENT) {
                if depth { 32 } else { 16 }
            } else {
                0
            };
            let info = vk::ImageCreateInfo {
                s_type: 14,
                image_type: 1,
                format: format(desc.format),
                extent: vk::Extent3D {
                    width: desc.size.width,
                    height: desc.size.height,
                    depth: 1,
                },
                mip_levels: desc.mip_level_count,
                array_layers: desc.size.depth_or_array_layers,
                samples: 1,
                usage,
                ..Default::default()
            };
            let mut image = 0;
            check(call!(
                d.instance,
                CreateImage,
                d.handle,
                &info,
                ptr::null(),
                &mut image
            ))?;
            let mut requirements = vk::MemoryRequirements::default();
            call!(
                d.instance,
                GetImageMemoryRequirements,
                d.handle,
                image,
                &mut requirements
            );
            let (memory, offset) = match d.allocate(requirements, 1, false) {
                Ok(value) => value,
                Err(e) => {
                    call!(d.instance, DestroyImage, d.handle, image, ptr::null());
                    return Err(e);
                }
            };
            let mut texture = Texture {
                image,
                view: 0,
                memory: Some(memory),
                #[cfg(feature = "window")]
                swap: None,
                device: d.clone(),
                layout: Cell::new(0),
                mips: desc.mip_level_count,
                layers: desc.size.depth_or_array_layers,
                depth,
                #[cfg(feature = "window")]
                index: 0,
            };
            check(call!(
                d.instance,
                BindImageMemory,
                d.handle,
                image,
                texture
                    .memory
                    .as_ref()
                    .expect("texture memory allocated")
                    .handle,
                offset
            ))?;
            // Transfer-only images cannot have Vulkan image views.
            if usage & (4 | 16 | 32) != 0 {
                texture.view = self.view(image, desc.format, texture.mips, texture.layers)?;
            }
            Ok(texture)
        }
    }

    pub(super) fn sampler(&self, desc: &SamplerDescriptor<'_>) -> Result<Sampler> {
        let info = vk::SamplerCreateInfo {
            s_type: 31,
            mag_filter: u32::from(desc.mag_filter == FilterMode::Linear),
            min_filter: u32::from(desc.min_filter == FilterMode::Linear),
            mipmap_mode: u32::from(desc.mipmap_filter == FilterMode::Linear),
            address_mode_u: if desc.address_mode_u == AddressMode::Repeat {
                0
            } else {
                2
            },
            address_mode_v: if desc.address_mode_v == AddressMode::Repeat {
                0
            } else {
                2
            },
            address_mode_w: if desc.address_mode_w == AddressMode::Repeat {
                0
            } else {
                2
            },
            min_lod: desc.lod_min_clamp,
            max_lod: desc.lod_max_clamp,
            ..Default::default()
        };
        let mut handle = 0;
        unsafe {
            check(call!(
                self.device.instance,
                CreateSampler,
                self.device.handle,
                &info,
                ptr::null(),
                &mut handle
            ))?;
        }
        Ok(Sampler {
            handle,
            device: self.device.clone(),
        })
    }

    fn render_pass(
        &self,
        fmt: TextureFormat,
        clear_color: bool,
        has_depth: bool,
        clear_depth: bool,
        store_depth: bool,
    ) -> Result<u64> {
        let key = RenderPassKey {
            format: format(fmt),
            clear_color,
            has_depth,
            clear_depth: has_depth && clear_depth,
            store_depth: has_depth && store_depth,
        };
        let mut cache = self.device.render_passes.borrow_mut();
        if let Some(&render) = cache.get(&key) {
            return Ok(render);
        }
        unsafe {
            let d = &self.device;
            let mut render = 0;
            let attachments = [
                vk::AttachmentDescription {
                    format: format(fmt),
                    samples: 1,
                    load_op: u32::from(clear_color),
                    store_op: 0,
                    stencil_load_op: 2,
                    stencil_store_op: 1,
                    initial_layout: 2,
                    final_layout: 2,
                    ..Default::default()
                },
                vk::AttachmentDescription {
                    format: 126,
                    samples: 1,
                    load_op: u32::from(clear_depth),
                    store_op: u32::from(!store_depth),
                    stencil_load_op: 2,
                    stencil_store_op: 1,
                    initial_layout: 3,
                    final_layout: 3,
                    ..Default::default()
                },
            ];
            let color_ref = vk::AttachmentReference {
                attachment: 0,
                layout: 2,
            };
            let depth_ref = vk::AttachmentReference {
                attachment: 1,
                layout: 3,
            };
            let subpass = vk::SubpassDescription {
                color_attachment_count: 1,
                p_color_attachments: &color_ref,
                p_depth_stencil_attachment: if has_depth { &depth_ref } else { ptr::null() },
                ..Default::default()
            };
            let info = vk::RenderPassCreateInfo {
                s_type: 38,
                attachment_count: if has_depth { 2 } else { 1 },
                p_attachments: attachments.as_ptr(),
                subpass_count: 1,
                p_subpasses: &subpass,
                ..Default::default()
            };
            check(call!(
                d.instance,
                CreateRenderPass,
                d.handle,
                &info,
                ptr::null(),
                &mut render
            ))?;
            cache.insert(key, render);
            Ok(render)
        }
    }

    pub(super) fn pipeline(&self, desc: &RenderPipelineDescriptor<'_>) -> Result<Pipeline> {
        unsafe {
            let d = &self.device;
            let mut pipeline = Pipeline {
                handle: 0,
                render: 0,
                device: d.clone(),
            };
            let fragment = desc.fragment.as_ref().expect("fragment stage validated");
            let color = fragment.targets[0].expect("color target validated");
            pipeline.render = self.render_pass(
                color.format,
                true,
                desc.depth_stencil.is_some(),
                desc.depth_stencil.is_some(),
                true,
            )?;
            let mut modules = ShaderModules {
                device: d,
                handles: [0; 2],
            };
            for (index, shader) in [desc.vertex.module, fragment.module].iter().enumerate() {
                let NativeSource::SpirV(words) = &shader.source else {
                    return Err(Error("Vulkan requires SPIR-V".into()));
                };
                let info = vk::ShaderModuleCreateInfo {
                    s_type: 16,
                    code_size: words.len() * 4,
                    p_code: words.as_ptr(),
                    ..Default::default()
                };
                check(call!(
                    d.instance,
                    CreateShaderModule,
                    d.handle,
                    &info,
                    ptr::null(),
                    &mut modules.handles[index]
                ))?;
            }
            let names = [
                std::ffi::CString::new(desc.vertex.module.entry(desc.vertex.entry_point))
                    .expect("WGSL entry point contains no NUL"),
                std::ffi::CString::new(fragment.module.entry(fragment.entry_point))
                    .expect("WGSL entry point contains no NUL"),
            ];
            let stages = [
                vk::PipelineShaderStageCreateInfo {
                    s_type: 18,
                    stage: 1,
                    module: modules.handles[0],
                    p_name: names[0].as_ptr(),
                    ..Default::default()
                },
                vk::PipelineShaderStageCreateInfo {
                    s_type: 18,
                    stage: 16,
                    module: modules.handles[1],
                    p_name: names[1].as_ptr(),
                    ..Default::default()
                },
            ];
            let layout = desc.vertex.buffers.first().and_then(Option::as_ref);
            let binding = vk::VertexInputBindingDescription {
                binding: 0,
                stride: layout.map_or(0, |l| l.array_stride as u32),
                input_rate: u32::from(
                    layout.is_some_and(|layout| layout.step_mode == VertexStepMode::Instance),
                ),
            };
            let attributes = layout
                .into_iter()
                .flat_map(|layout| layout.attributes)
                .map(|attribute| vk::VertexInputAttributeDescription {
                    location: attribute.shader_location,
                    binding: 0,
                    format: match attribute.format {
                        VertexFormat::Float32x2 => 103,
                        VertexFormat::Float32x3 => 106,
                        VertexFormat::Uint32x2 => 101,
                        VertexFormat::Float32x4 => 109,
                        VertexFormat::Uint8x4 => 41,
                        VertexFormat::Unorm8x4 => 37,
                        VertexFormat::Sint16x4 => 96,
                    },
                    offset: attribute.offset as u32,
                })
                .collect::<Vec<_>>();
            let vertex = vk::PipelineVertexInputStateCreateInfo {
                s_type: 19,
                vertex_binding_description_count: u32::from(layout.is_some()),
                p_vertex_binding_descriptions: &binding,
                vertex_attribute_description_count: attributes.len() as u32,
                p_vertex_attribute_descriptions: attributes.as_ptr(),
                ..Default::default()
            };
            let assembly = vk::PipelineInputAssemblyStateCreateInfo {
                s_type: 20,
                topology: 3,
                ..Default::default()
            };
            let viewport = vk::PipelineViewportStateCreateInfo {
                s_type: 22,
                viewport_count: 1,
                scissor_count: 1,
                ..Default::default()
            };
            let raster = vk::PipelineRasterizationStateCreateInfo {
                s_type: 23,
                cull_mode: if desc.primitive.cull_mode.is_some() {
                    2
                } else {
                    0
                },
                front_face: 0,
                line_width: 1.,
                ..Default::default()
            };
            let multisample = vk::PipelineMultisampleStateCreateInfo {
                s_type: 24,
                rasterization_samples: 1,
                ..Default::default()
            };
            let depth = desc.depth_stencil;
            let depth = vk::PipelineDepthStencilStateCreateInfo {
                s_type: 25,
                depth_test_enable: u32::from(depth.is_some()),
                depth_write_enable: u32::from(
                    depth.is_some_and(|depth| depth.depth_write_enabled.unwrap_or(false)),
                ),
                depth_compare_op: depth
                    .and_then(|depth| depth.depth_compare)
                    .unwrap_or(CompareFunction::LessEqual) as u32
                    - 1,
                ..Default::default()
            };
            let state = color.blend.unwrap_or(BlendState::REPLACE);
            let blend_attachment = vk::PipelineColorBlendAttachmentState {
                blend_enable: u32::from(color.blend.is_some()),
                src_color_blend_factor: blend_factor(state.color.src_factor),
                dst_color_blend_factor: blend_factor(state.color.dst_factor),
                color_blend_op: state.color.operation as u32,
                src_alpha_blend_factor: blend_factor(state.alpha.src_factor.for_alpha()),
                dst_alpha_blend_factor: blend_factor(state.alpha.dst_factor.for_alpha()),
                alpha_blend_op: state.alpha.operation as u32,
                color_write_mask: 15,
            };
            let blend = vk::PipelineColorBlendStateCreateInfo {
                s_type: 26,
                attachment_count: 1,
                p_attachments: &blend_attachment,
                ..Default::default()
            };
            let states = [0, 1];
            let dynamic = vk::PipelineDynamicStateCreateInfo {
                s_type: 27,
                dynamic_state_count: 2,
                p_dynamic_states: states.as_ptr(),
                ..Default::default()
            };
            let info = vk::GraphicsPipelineCreateInfo {
                s_type: 28,
                stage_count: 2,
                p_stages: stages.as_ptr(),
                p_vertex_input_state: &vertex,
                p_input_assembly_state: &assembly,
                p_viewport_state: &viewport,
                p_rasterization_state: &raster,
                p_multisample_state: &multisample,
                p_depth_stencil_state: &depth,
                p_color_blend_state: &blend,
                p_dynamic_state: &dynamic,
                layout: d.pipeline_layout,
                render_pass: pipeline.render,
                base_pipeline_index: -1,
                ..Default::default()
            };
            let result = call!(
                d.instance,
                CreateGraphicsPipelines,
                d.handle,
                0,
                1,
                &info,
                ptr::null(),
                &mut pipeline.handle
            );
            drop(modules);
            check(result)?;
            Ok(pipeline)
        }
    }

    unsafe fn transition(&self, command: vk::CommandBuffer, texture: &Texture, layout: u32) {
        let old = texture.layout.replace(layout);
        // Repeated transfer writes may overlap even without a layout change.
        if old == layout && layout != 7 {
            return;
        }
        let barrier = vk::ImageMemoryBarrier {
            s_type: 45,
            src_access_mask: if old == 0 { 0 } else { 0x18000 },
            dst_access_mask: 0x18000,
            old_layout: old,
            new_layout: layout,
            src_queue_family_index: u32::MAX,
            dst_queue_family_index: u32::MAX,
            image: texture.image,
            subresource_range: vk::ImageSubresourceRange {
                aspect_mask: if texture.depth { 2 } else { 1 },
                level_count: texture.mips,
                layer_count: texture.layers,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            call!(
                self.device.instance,
                CmdPipelineBarrier,
                command,
                0x10000,
                0x10000,
                0,
                0,
                ptr::null(),
                0,
                ptr::null(),
                1,
                &barrier
            );
        }
    }

    pub(super) fn submit(&self, uploads: Vec<Upload>, passes: Vec<Pass>) -> Result<()> {
        unsafe {
            let d = &self.device;
            let mut frames = self.frames.borrow_mut();
            let frame = &mut frames[self.slot.get()];
            self.wait(frame, u64::MAX)?;
            let info = vk::CommandBufferBeginInfo {
                s_type: 42,
                flags: 1,
                ..Default::default()
            };
            check(call!(d.instance, BeginCommandBuffer, frame.command, &info))?;
            // Serialize transfers with previous frames before modifying shared resources.
            let memory = vk::MemoryBarrier {
                s_type: 46,
                src_access_mask: 0x18000,
                dst_access_mask: 0x18000,
                ..Default::default()
            };
            call!(
                d.instance,
                CmdPipelineBarrier,
                frame.command,
                0x10000,
                0x10000,
                0,
                1,
                &memory,
                0,
                ptr::null(),
                0,
                ptr::null()
            );
            let total: usize = uploads
                .iter()
                .map(|u| match u {
                    Upload::Buffer { data, .. } | Upload::Texture { data, .. } => {
                        (data.len() + 3) & !3
                    }
                })
                .sum();
            if total > 0 {
                let staging = match frame.staging.pop() {
                    Some(buffer) if buffer.capacity >= total as u64 => buffer,
                    _ => d.buffer(total as u64, true)?,
                };
                let mut offset = 0;
                let mut written = std::collections::HashSet::new();
                for upload in &uploads {
                    let data = match upload {
                        Upload::Buffer { data, .. } | Upload::Texture { data, .. } => data,
                    };
                    ptr::copy_nonoverlapping(data.as_ptr(), staging.mapped.add(offset), data.len());
                    match upload {
                        Upload::Buffer {
                            buffer,
                            offset: destination,
                            ..
                        } => {
                            if !written.insert(buffer.inner.native.handle) {
                                call!(
                                    d.instance,
                                    CmdPipelineBarrier,
                                    frame.command,
                                    0x10000,
                                    0x10000,
                                    0,
                                    1,
                                    &memory,
                                    0,
                                    ptr::null(),
                                    0,
                                    ptr::null()
                                );
                            }
                            let region = vk::BufferCopy {
                                src_offset: offset as u64,
                                dst_offset: *destination,
                                size: data.len() as u64,
                            };
                            call!(
                                d.instance,
                                CmdCopyBuffer,
                                frame.command,
                                staging.handle,
                                buffer.inner.native.handle,
                                1,
                                &region
                            );
                        }
                        Upload::Texture {
                            texture,
                            mip,
                            origin,
                            size,
                            ..
                        } => {
                            self.transition(frame.command, &texture.inner.native, 7);
                            let region = vk::BufferImageCopy {
                                buffer_offset: offset as u64,
                                image_subresource: vk::ImageSubresourceLayers {
                                    aspect_mask: 1,
                                    mip_level: *mip,
                                    base_array_layer: origin.z,
                                    layer_count: size.depth_or_array_layers,
                                },
                                image_offset: vk::Offset3D {
                                    x: origin.x as i32,
                                    y: origin.y as i32,
                                    z: 0,
                                },
                                image_extent: vk::Extent3D {
                                    width: size.width,
                                    height: size.height,
                                    depth: 1,
                                },
                                ..Default::default()
                            };
                            call!(
                                d.instance,
                                CmdCopyBufferToImage,
                                frame.command,
                                staging.handle,
                                texture.inner.native.image,
                                7,
                                1,
                                &region
                            );
                        }
                    }
                    offset += (data.len() + 3) & !3;
                }
                frame.staging.push(staging);
            }
            let mut sets = HashMap::new();
            for pass in &passes {
                call!(
                    d.instance,
                    CmdPipelineBarrier,
                    frame.command,
                    0x10000,
                    0x10000,
                    0,
                    1,
                    &memory,
                    0,
                    ptr::null(),
                    0,
                    ptr::null()
                );
                // Load/store operations belong to the pass, not its pipelines.
                // Vulkan render-pass compatibility ignores these operations.
                let render = self.render_pass(
                    pass.color.texture.inner.format,
                    pass.clear.is_some(),
                    pass.depth.is_some(),
                    pass.clear_depth.is_some(),
                    pass.store_depth,
                )?;
                for draw in &pass.draws {
                    if let Some(group) = &draw.group {
                        for (_, resource) in group.entries.iter() {
                            if let Resource::Texture(view) = resource {
                                self.transition(frame.command, &view.texture.inner.native, 5);
                            }
                        }
                    }
                }
                self.transition(frame.command, &pass.color.texture.inner.native, 2);
                if let Some(depth) = &pass.depth {
                    self.transition(frame.command, &depth.texture.inner.native, 3);
                }
                let mut views = vec![pass.color.texture.inner.native.view];
                if let Some(depth) = &pass.depth {
                    views.push(depth.texture.inner.native.view);
                }
                let size = pass.color.texture.inner.size;
                let info = vk::FramebufferCreateInfo {
                    s_type: 37,
                    render_pass: render,
                    attachment_count: views.len() as u32,
                    p_attachments: views.as_ptr(),
                    width: size.width,
                    height: size.height,
                    layers: 1,
                    ..Default::default()
                };
                let mut framebuffer = 0;
                check(call!(
                    d.instance,
                    CreateFramebuffer,
                    d.handle,
                    &info,
                    ptr::null(),
                    &mut framebuffer
                ))?;
                frame.framebuffers.push(framebuffer);
                let clear = pass.clear.unwrap_or(Color::BLACK);
                let clears = [
                    vk::ClearValue {
                        color: vk::ClearColorValue {
                            float32: [
                                clear.r as f32,
                                clear.g as f32,
                                clear.b as f32,
                                clear.a as f32,
                            ],
                        },
                    },
                    vk::ClearValue {
                        depth_stencil: vk::ClearDepthStencilValue {
                            depth: pass.clear_depth.unwrap_or(1.),
                            stencil: 0,
                        },
                    },
                ];
                let info = vk::RenderPassBeginInfo {
                    s_type: 43,
                    render_pass: render,
                    framebuffer,
                    render_area: vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                            width: size.width,
                            height: size.height,
                        },
                    },
                    clear_value_count: views.len() as u32,
                    p_clear_values: clears.as_ptr(),
                    ..Default::default()
                };
                call!(d.instance, CmdBeginRenderPass, frame.command, &info, 0);
                for draw in &pass.draws {
                    let pipeline = &draw.pipeline.native;
                    call!(
                        d.instance,
                        CmdBindPipeline,
                        frame.command,
                        0,
                        pipeline.handle
                    );
                    if let Some(group) = &draw.group {
                        let key = Rc::as_ptr(&group.entries);
                        let set = if let Some(&set) = sets.get(&key) {
                            set
                        } else {
                            let info = vk::DescriptorSetAllocateInfo {
                                s_type: 34,
                                descriptor_pool: frame.descriptors,
                                descriptor_set_count: 1,
                                p_set_layouts: &d.set_layout,
                                ..Default::default()
                            };
                            let mut set = 0;
                            check(call!(
                                d.instance,
                                AllocateDescriptorSets,
                                d.handle,
                                &info,
                                &mut set
                            ))?;
                            for (binding, resource) in group.entries.iter() {
                                let mut buffer = vk::DescriptorBufferInfo::default();
                                let mut image = vk::DescriptorImageInfo::default();
                                let ty = match resource {
                                    Resource::Buffer(b) => {
                                        buffer.buffer = b.inner.native.handle;
                                        buffer.range = b.inner.size;
                                        // Uniform buffers use binding zero, storage buffers three.
                                        if *binding == 0 { 6 } else { 7 }
                                    }
                                    Resource::Texture(t) => {
                                        image.image_view = t.texture.inner.native.view;
                                        image.image_layout = 5;
                                        2
                                    }
                                    Resource::Sampler(s) => {
                                        image.sampler = s.native.handle;
                                        0
                                    }
                                };
                                let write = vk::WriteDescriptorSet {
                                    s_type: 35,
                                    dst_set: set,
                                    dst_binding: *binding,
                                    descriptor_count: 1,
                                    descriptor_type: ty,
                                    p_buffer_info: &buffer,
                                    p_image_info: &image,
                                    ..Default::default()
                                };
                                call!(
                                    d.instance,
                                    UpdateDescriptorSets,
                                    d.handle,
                                    1,
                                    &write,
                                    0,
                                    ptr::null()
                                );
                            }
                            sets.insert(key, set);
                            set
                        };
                        call!(
                            d.instance,
                            CmdBindDescriptorSets,
                            frame.command,
                            0,
                            d.pipeline_layout,
                            0,
                            1,
                            &set,
                            0,
                            ptr::null()
                        );
                    }
                    if let Some((buffer, range)) = &draw.vertex {
                        call!(
                            d.instance,
                            CmdBindVertexBuffers,
                            frame.command,
                            0,
                            1,
                            &buffer.inner.native.handle,
                            &range.start
                        );
                    }
                    let v = draw.viewport;
                    // Naga's SPIR-V writer flips clip-space Y to Vulkan's convention.
                    let viewport = vk::Viewport {
                        x: v[0],
                        y: v[1],
                        width: v[2],
                        height: v[3],
                        min_depth: v[4],
                        max_depth: v[5],
                    };
                    call!(d.instance, CmdSetViewport, frame.command, 0, 1, &viewport);
                    let s = draw.scissor;
                    let scissor = vk::Rect2D {
                        offset: vk::Offset2D {
                            x: s[0] as i32,
                            y: s[1] as i32,
                        },
                        extent: vk::Extent2D {
                            width: s[2],
                            height: s[3],
                        },
                    };
                    call!(d.instance, CmdSetScissor, frame.command, 0, 1, &scissor);
                    match &draw.kind {
                        DrawKind::Direct(vertices) => call!(
                            d.instance,
                            CmdDraw,
                            frame.command,
                            vertices.len() as u32,
                            draw.instances.len() as u32,
                            vertices.start,
                            draw.instances.start
                        ),
                        DrawKind::Indexed {
                            indices,
                            base_vertex,
                            binding,
                        } => {
                            call!(
                                d.instance,
                                CmdBindIndexBuffer,
                                frame.command,
                                binding.buffer.inner.native.handle,
                                binding.range.start,
                                u32::from(binding.format == IndexFormat::Uint32)
                            );
                            call!(
                                d.instance,
                                CmdDrawIndexed,
                                frame.command,
                                indices.len() as u32,
                                draw.instances.len() as u32,
                                indices.start,
                                *base_vertex,
                                draw.instances.start
                            );
                        }
                    }
                }
                call!(d.instance, CmdEndRenderPass, frame.command);
                #[cfg(feature = "window")]
                if pass.color.texture.inner.native.swap.is_some() {
                    self.transition(frame.command, &pass.color.texture.inner.native, 1000001002);
                }
            }
            check(call!(d.instance, EndCommandBuffer, frame.command))?;
            check(call!(d.instance, ResetFences, d.handle, 1, &frame.fence))?;
            #[cfg(feature = "window")]
            let wait = self.take_acquisition_wait();
            #[cfg(not(feature = "window"))]
            let wait = false;
            let stage = 0x10000u32;
            let info = vk::SubmitInfo {
                s_type: 4,
                wait_semaphore_count: u32::from(wait),
                p_wait_semaphores: &frame.available,
                p_wait_dst_stage_mask: &stage,
                command_buffer_count: 1,
                p_command_buffers: &frame.command,
                ..Default::default()
            };
            check(call!(
                d.instance,
                QueueSubmit,
                d.queue,
                1,
                &info,
                frame.fence
            ))?;
            frame.busy = true;
            frame.uploads = uploads;
            frame.passes = passes;
            Ok(())
        }
    }

    #[cfg(feature = "image")]
    pub(super) fn read_texture(
        &self,
        texture: &Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        unsafe {
            let d = &self.device;
            let length = u64::from(width) * u64::from(height) * 4;
            let buffer = d.buffer(length, true)?;
            let mut frames = self.frames.borrow_mut();
            let frame = &mut frames[self.slot.get()];
            self.wait(frame, u64::MAX)?;
            let begin = vk::CommandBufferBeginInfo {
                s_type: 42,
                flags: 1,
                ..Default::default()
            };
            check(call!(d.instance, BeginCommandBuffer, frame.command, &begin))?;
            self.transition(frame.command, texture, 6); // VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL
            let region = vk::BufferImageCopy {
                image_subresource: vk::ImageSubresourceLayers {
                    aspect_mask: 1,
                    layer_count: 1,
                    ..Default::default()
                },
                image_extent: vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                },
                ..Default::default()
            };
            call!(
                d.instance,
                CmdCopyImageToBuffer,
                frame.command,
                texture.image,
                6,
                buffer.handle,
                1,
                &region
            );
            // Make the transfer write visible to the host mapping below.
            let host = vk::MemoryBarrier {
                s_type: 46,
                src_access_mask: 0x1000, // VK_ACCESS_TRANSFER_WRITE_BIT
                dst_access_mask: 0x2000, // VK_ACCESS_HOST_READ_BIT
                ..Default::default()
            };
            call!(
                d.instance,
                CmdPipelineBarrier,
                frame.command,
                0x1000, // VK_PIPELINE_STAGE_TRANSFER_BIT
                0x4000, // VK_PIPELINE_STAGE_HOST_BIT
                0,
                1,
                &host,
                0,
                ptr::null(),
                0,
                ptr::null()
            );
            check(call!(d.instance, EndCommandBuffer, frame.command))?;
            check(call!(d.instance, ResetFences, d.handle, 1, &frame.fence))?;
            let submit = vk::SubmitInfo {
                s_type: 4,
                command_buffer_count: 1,
                p_command_buffers: &frame.command,
                ..Default::default()
            };
            check(call!(
                d.instance,
                QueueSubmit,
                d.queue,
                1,
                &submit,
                frame.fence
            ))?;
            frame.busy = true;
            check(call!(
                d.instance,
                WaitForFences,
                d.handle,
                1,
                &frame.fence,
                1,
                u64::MAX
            ))?;
            Ok(std::slice::from_raw_parts(buffer.mapped, length as usize).to_vec())
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            call!(self.device.instance, DeviceWaitIdle, self.device.handle);
        }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            let d = &self.device;
            if self.busy {
                call!(
                    d.instance,
                    WaitForFences,
                    d.handle,
                    1,
                    &self.fence,
                    1,
                    u64::MAX
                );
            }
            for framebuffer in self.framebuffers.drain(..) {
                call!(
                    d.instance,
                    DestroyFramebuffer,
                    d.handle,
                    framebuffer,
                    ptr::null()
                );
            }
            call!(
                d.instance,
                DestroyDescriptorPool,
                d.handle,
                self.descriptors,
                ptr::null()
            );
            call!(
                d.instance,
                DestroyCommandPool,
                d.handle,
                self.pool,
                ptr::null()
            );
            call!(d.instance, DestroyFence, d.handle, self.fence, ptr::null());
            call!(
                d.instance,
                DestroySemaphore,
                d.handle,
                self.available,
                ptr::null()
            );
        }
    }
}

struct ShaderModules<'a> {
    device: &'a NativeDevice,
    handles: [u64; 2],
}

impl Drop for ShaderModules<'_> {
    fn drop(&mut self) {
        unsafe {
            for handle in self.handles {
                call!(
                    self.device.instance,
                    DestroyShaderModule,
                    self.device.handle,
                    handle,
                    ptr::null()
                );
            }
        }
    }
}
