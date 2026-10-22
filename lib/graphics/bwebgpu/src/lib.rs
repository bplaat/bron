/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The README examples need the default `window` feature.
#![cfg_attr(feature = "window", doc = include_str!("../README.md"))]
#![allow(missing_docs)]
#![allow(unsafe_code)]
#![allow(clippy::undocumented_unsafe_blocks)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::cell::{Cell, RefCell, RefMut};
use std::ops::{Bound, Range, RangeBounds};
use std::rc::{Rc, Weak};

#[cfg(feature = "window")]
pub use crate::surface::*;
pub use crate::types::*;

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd",
    test
))]
mod allocation;
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[path = "platforms/metal/mod.rs"]
mod platform;
// Vulkan, with GTK on Linux and the BSDs.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
#[path = "platforms/vulkan/mod.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "platforms/directx/mod.rs"]
mod platform;
#[cfg(feature = "window")]
mod surface;
mod types;

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "windows",
    target_os = "android"
)))]
compile_error!("This crate supports macOS, iOS, Windows, Linux, the BSDs, and Android only");

#[cfg(not(any(feature = "image", feature = "window")))]
compile_error!("Enable the `image` feature, the `window` feature, or both");

#[derive(Debug)]
pub struct Error(pub(crate) String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn require(value: bool, message: &str) {
    assert!(value, "{message}");
}

#[derive(Default)]
pub struct Instance {
    #[cfg(feature = "window")]
    factory: RefCell<Option<platform::Factory>>,
}

pub struct Adapter {
    shared: Rc<Shared>,
    #[cfg(feature = "window")]
    formats: Vec<TextureFormat>,
}

struct Shared {
    native: Rc<platform::Context>,
    uploads: RefCell<Vec<Upload>>,
}

pub struct Device {
    shared: Rc<Shared>,
}

pub struct Queue {
    shared: Rc<Shared>,
}

impl Instance {
    pub fn request_adapter(&self, options: &RequestAdapterOptions<'_>) -> Result<Adapter> {
        #[cfg(feature = "window")]
        if let Some(surface) = options.compatible_surface {
            let native = platform::Context::with_surface(&surface.native)?;
            let formats = native.formats(&surface.native)?;
            return Ok(Adapter {
                formats,
                ..Adapter::new(native)
            });
        }
        #[cfg(not(feature = "window"))]
        let _ = options;
        Ok(Adapter::new(platform::Context::new()?))
    }
}

impl Adapter {
    fn new(native: Rc<platform::Context>) -> Self {
        Self {
            shared: Rc::new(Shared {
                native,
                uploads: RefCell::new(Vec::new()),
            }),
            #[cfg(feature = "window")]
            formats: Vec::new(),
        }
    }

    pub fn request_device(&self, _: &DeviceDescriptor) -> Result<(Device, Queue)> {
        Ok((
            Device {
                shared: self.shared.clone(),
            },
            Queue {
                shared: self.shared.clone(),
            },
        ))
    }
}

#[derive(Clone)]
pub struct Buffer {
    inner: Rc<BufferInner>,
}

struct BufferInner {
    native: platform::Buffer,
    owner: Weak<Shared>,
    size: u64,
    usage: BufferUsages,
    mapped: Cell<bool>,
    data: RefCell<Vec<u8>>,
}

pub struct BufferSlice<'a> {
    buffer: &'a Buffer,
    range: Range<u64>,
}

pub struct BufferViewMut<'a> {
    data: RefMut<'a, Vec<u8>>,
    range: Range<usize>,
}

impl BufferViewMut<'_> {
    pub fn slice(&mut self, range: Range<usize>) -> &mut [u8] {
        require(
            range.start <= range.end && range.end <= self.range.len(),
            "Mapped slice out of bounds",
        );
        &mut self.data[self.range.start + range.start..self.range.start + range.end]
    }
}

impl std::ops::Deref for BufferViewMut<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

impl std::ops::DerefMut for BufferViewMut<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.data[self.range.clone()]
    }
}

fn bounds(range: impl RangeBounds<u64>, size: u64) -> Range<u64> {
    let start = match range.start_bound() {
        Bound::Included(&n) => n,
        Bound::Excluded(&n) => n.checked_add(1).expect("Range overflow"),
        Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        Bound::Included(&n) => n.checked_add(1).expect("Range overflow"),
        Bound::Excluded(&n) => n,
        Bound::Unbounded => size,
    };
    require(start <= end && end <= size, "Buffer range out of bounds");
    start..end
}

impl Buffer {
    pub fn slice(&self, range: impl RangeBounds<u64>) -> BufferSlice<'_> {
        BufferSlice {
            buffer: self,
            range: bounds(range, self.inner.size),
        }
    }

    pub const fn as_entire_binding(&self) -> BindingResource<'_> {
        BindingResource::Buffer(BufferBinding { buffer: self })
    }

    pub fn unmap(&self) {
        let mut data = self
            .inner
            .data
            .try_borrow_mut()
            .expect("Mapped view is still alive");
        require(self.inner.mapped.replace(false), "Buffer is not mapped");
        if !data.is_empty() {
            let shared = self.inner.owner.upgrade().expect("Device has been dropped");
            shared.uploads.borrow_mut().push(Upload::Buffer {
                buffer: self.clone(),
                offset: 0,
                data: std::mem::take(&mut *data),
            });
        }
    }
}

impl<'a> BufferSlice<'a> {
    pub fn get_mapped_range_mut(&self) -> Result<BufferViewMut<'a>> {
        if !self.buffer.inner.mapped.get() {
            return Err(Error("Buffer is not mapped".into()));
        }
        Ok(BufferViewMut {
            data: self
                .buffer
                .inner
                .data
                .try_borrow_mut()
                .map_err(|_| Error("Buffer already borrowed".into()))?,
            range: self.range.start as usize..self.range.end as usize,
        })
    }
}

#[derive(Clone)]
pub struct Texture {
    inner: Rc<TextureInner>,
}

struct TextureInner {
    native: platform::Texture,
    owner: Weak<Shared>,
    size: Extent3d,
    mips: u32,
    format: TextureFormat,
    usage: TextureUsages,
    _window: Option<Rc<dyn std::any::Any>>,
}

#[derive(Clone)]
pub struct TextureView {
    texture: Texture,
    dimension: TextureViewDimension,
}

/// A render target whose pixels can be read into an image bitmap.
#[cfg(feature = "image")]
pub struct BitmapTarget {
    texture: Texture,
}

#[cfg(feature = "image")]
impl BitmapTarget {
    pub fn create_view(&self) -> TextureView {
        self.texture.create_view(&TextureViewDescriptor::default())
    }

    /// Wait for submitted rendering and return tightly packed RGBA8 pixels.
    pub fn read(&self, queue: &Queue) -> Result<image::Bitmap> {
        queue.shared.check(&self.texture.inner.owner);
        let size = self.texture.inner.size;
        let bytes = queue.shared.native.read_texture(
            &self.texture.inner.native,
            size.width,
            size.height,
        )?;
        image::Bitmap::new(size.width, size.height, bytes).map_err(|error| Error(error.to_string()))
    }
}

impl Texture {
    pub fn create_view(&self, desc: &TextureViewDescriptor) -> TextureView {
        let dimension = desc
            .dimension
            .unwrap_or(if self.inner.size.depth_or_array_layers > 1 {
                TextureViewDimension::D2Array
            } else {
                TextureViewDimension::D2
            });
        require(
            matches!(dimension, TextureViewDimension::D2Array)
                == (self.inner.size.depth_or_array_layers > 1),
            "Texture view dimension does not match its layer count",
        );
        TextureView {
            texture: self.clone(),
            dimension,
        }
    }
}

#[derive(Clone)]
pub struct Sampler {
    native: Rc<platform::Sampler>,
    owner: Weak<Shared>,
}

#[derive(Clone)]
pub struct BindGroupLayout {
    entries: Rc<Vec<BindGroupLayoutEntry>>,
}

#[derive(Clone)]
pub struct PipelineLayout {
    group: Option<BindGroupLayout>,
}

#[derive(Clone)]
enum Resource {
    Buffer(Buffer),
    Texture(TextureView),
    Sampler(Sampler),
}

#[derive(Clone)]
pub struct BindGroup {
    owner: Weak<Shared>,
    entries: Rc<Vec<(u32, Resource)>>,
    layout: BindGroupLayout,
}

pub struct ShaderModule {
    source: NativeSource,
    entries: Vec<(String, String)>,
}

#[allow(dead_code)] // Only the target backend reads its native shader representation.
enum NativeSource {
    Text(String),
    Binary(Vec<(String, Vec<u8>)>),
    SpirV(Vec<u32>),
}

impl ShaderModule {
    fn entry(&self, name: Option<&str>) -> &str {
        let name = name.expect("Explicit shader entry point required");
        &self
            .entries
            .iter()
            .find(|(original, _)| original == name)
            .expect("Unknown shader entry point")
            .1
    }

    #[cfg(any(target_os = "macos", target_os = "ios", test))]
    fn binary(&self, name: Option<&str>) -> Option<&[u8]> {
        let NativeSource::Binary(binaries) = &self.source else {
            return None;
        };
        let entry = self.entry(name);
        binaries
            .iter()
            .find(|(name, _)| name == entry)
            .or_else(|| binaries.iter().find(|(name, _)| name.is_empty()))
            .map(|(_, bytes)| bytes.as_slice())
    }

    #[cfg(target_os = "windows")]
    fn binary_for(&self, backend: &str, name: Option<&str>) -> Option<&[u8]> {
        let NativeSource::Binary(binaries) = &self.source else {
            return None;
        };
        let entry = self.entry(name);
        let qualified = format!("{backend}:{entry}");
        binaries
            .iter()
            .find(|(name, _)| name == &qualified)
            .or_else(|| binaries.iter().find(|(name, _)| name == entry))
            .or_else(|| binaries.iter().find(|(name, _)| name.is_empty()))
            .map(|(_, bytes)| bytes.as_slice())
    }
}

#[derive(Clone)]
pub struct RenderPipeline {
    native: Rc<platform::Pipeline>,
    layout: PipelineLayout,
    vertex: Option<(u64, VertexStepMode)>,
    format: TextureFormat,
    depth: bool,
    owner: Weak<Shared>,
}

impl Device {
    /// Create an offscreen RGBA8 render target for bitmap export.
    #[cfg(feature = "image")]
    pub fn create_bitmap_target(&self, width: u32, height: u32) -> BitmapTarget {
        BitmapTarget {
            texture: self.create_texture(&TextureDescriptor {
                label: None,
                size: Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8Unorm,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
                view_formats: &[],
            }),
        }
    }

    pub fn create_buffer(&self, desc: &BufferDescriptor<'_>) -> Buffer {
        require(desc.size <= usize::MAX as u64, "Buffer is too large");
        require(
            !desc.usage.contains(BufferUsages::UNIFORM)
                || !desc
                    .usage
                    .intersects(BufferUsages::VERTEX | BufferUsages::STORAGE),
            "Uniform buffers cannot also be vertex or storage buffers",
        );
        require(
            !desc.mapped_at_creation || desc.size.is_multiple_of(4),
            "Mapped buffer size requires four-byte alignment",
        );
        Buffer {
            inner: Rc::new(BufferInner {
                native: self
                    .shared
                    .native
                    .buffer(desc.size.max(4), desc.usage)
                    .expect("Create buffer"),
                owner: Rc::downgrade(&self.shared),
                size: desc.size,
                usage: desc.usage,
                mapped: Cell::new(desc.mapped_at_creation),
                data: RefCell::new(if desc.mapped_at_creation {
                    vec![0; desc.size as usize]
                } else {
                    Vec::new()
                }),
            }),
        }
    }

    pub fn create_texture(&self, desc: &TextureDescriptor<'_>) -> Texture {
        require(
            desc.size.width > 0 && desc.size.height > 0 && desc.size.depth_or_array_layers > 0,
            "Empty texture",
        );
        require(
            desc.sample_count == 1 && desc.view_formats.is_empty(),
            "Unsupported texture configuration",
        );
        require(
            desc.mip_level_count > 0
                && desc.mip_level_count
                    <= 32 - desc.size.width.max(desc.size.height).leading_zeros(),
            "Invalid mip count",
        );
        Texture {
            inner: Rc::new(TextureInner {
                native: self.shared.native.texture(desc).expect("Create texture"),
                owner: Rc::downgrade(&self.shared),
                size: desc.size,
                mips: desc.mip_level_count,
                format: desc.format,
                usage: desc.usage,
                _window: None,
            }),
        }
    }

    pub fn create_sampler(&self, desc: &SamplerDescriptor<'_>) -> Sampler {
        Sampler {
            native: Rc::new(self.shared.native.sampler(desc).expect("Create sampler")),
            owner: Rc::downgrade(&self.shared),
        }
    }

    pub fn create_bind_group_layout(
        &self,
        desc: &BindGroupLayoutDescriptor<'_>,
    ) -> BindGroupLayout {
        let mut entries = desc.entries.to_vec();
        entries.sort_by_key(|entry| entry.binding);
        for (i, entry) in entries.iter().enumerate() {
            require(
                entry.binding < 16 && entry.count.is_none(),
                "Unsupported binding",
            );
            require(
                i == 0 || entries[i - 1].binding != entry.binding,
                "Duplicate binding",
            );
            if let BindingType::Buffer {
                ty,
                has_dynamic_offset,
                ..
            } = entry.ty
            {
                require(
                    !has_dynamic_offset && ty != BufferBindingType::Storage { read_only: false },
                    "Unsupported buffer binding",
                );
            }
            match entry.ty {
                BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    ..
                } => require(entry.binding == 0, "Uniform buffers use binding zero"),
                BindingType::Texture { multisampled, .. } => require(
                    entry.binding == 1
                        && entry.visibility == ShaderStages::FRAGMENT
                        && !multisampled,
                    "Sampled textures use fragment binding one",
                ),
                BindingType::Sampler(_) => require(
                    entry.binding == 2 && entry.visibility == ShaderStages::FRAGMENT,
                    "Samplers use fragment binding two",
                ),
                BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    ..
                } => require(
                    entry.binding == 3 && entry.visibility == ShaderStages::VERTEX,
                    "Storage buffers use vertex binding three",
                ),
                BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: false },
                    ..
                } => unreachable!(),
            }
        }
        BindGroupLayout {
            entries: Rc::new(entries),
        }
    }

    pub fn create_pipeline_layout(&self, desc: &PipelineLayoutDescriptor<'_>) -> PipelineLayout {
        require(
            desc.bind_group_layouts.len() <= 1,
            "At most one bind group is supported",
        );
        PipelineLayout {
            group: desc
                .bind_group_layouts
                .first()
                .map(|layout| layout.expect("Missing bind group layout").clone()),
        }
    }

    pub fn create_bind_group(&self, desc: &BindGroupDescriptor<'_>) -> BindGroup {
        require(
            desc.entries.len() == desc.layout.entries.len(),
            "Bind group size mismatch",
        );
        let mut entries = Vec::new();
        for layout in desc.layout.entries.iter() {
            let entry = desc
                .entries
                .iter()
                .find(|entry| entry.binding == layout.binding)
                .expect("Missing binding");
            let resource = match (&entry.resource, layout.ty) {
                (
                    BindingResource::Buffer(binding),
                    BindingType::Buffer {
                        ty,
                        min_binding_size,
                        ..
                    },
                ) => {
                    self.shared.check(&binding.buffer.inner.owner);
                    let usage = if ty == BufferBindingType::Uniform {
                        BufferUsages::UNIFORM
                    } else {
                        BufferUsages::STORAGE
                    };
                    require(
                        binding.buffer.inner.usage.contains(usage),
                        "Buffer binding usage mismatch",
                    );
                    require(
                        binding.buffer.inner.size >= min_binding_size.map_or(1, |n| n.get()),
                        "Buffer binding too small",
                    );
                    Resource::Buffer(binding.buffer.clone())
                }
                (
                    BindingResource::TextureView(view),
                    BindingType::Texture {
                        view_dimension,
                        multisampled: false,
                        ..
                    },
                ) => {
                    self.shared.check(&view.texture.inner.owner);
                    require(
                        view.dimension == view_dimension
                            && view.texture.inner.format != TextureFormat::Depth32Float
                            && view
                                .texture
                                .inner
                                .usage
                                .contains(TextureUsages::TEXTURE_BINDING),
                        "Texture binding mismatch",
                    );
                    Resource::Texture((*view).clone())
                }
                (BindingResource::Sampler(sampler), BindingType::Sampler(_)) => {
                    self.shared.check(&sampler.owner);
                    Resource::Sampler((*sampler).clone())
                }
                _ => panic!("Binding type mismatch"),
            };
            entries.push((layout.binding, resource));
        }
        BindGroup {
            owner: Rc::downgrade(&self.shared),
            entries: Rc::new(entries),
            layout: desc.layout.clone(),
        }
    }

    pub fn create_shader_module(&self, desc: ShaderModuleDescriptor<'_>) -> ShaderModule {
        ShaderModule {
            source: match desc.source {
                ShaderSource::Text(text) => NativeSource::Text(text.into()),
                ShaderSource::Binary(binaries) => NativeSource::Binary(
                    binaries
                        .iter()
                        .map(|(entry, bytes)| ((*entry).into(), (*bytes).into()))
                        .collect(),
                ),
                ShaderSource::SpirV(words) => NativeSource::SpirV(words.into()),
            },
            entries: desc
                .entries
                .iter()
                .map(|(a, b)| ((*a).into(), (*b).into()))
                .collect(),
        }
    }

    pub fn create_render_pipeline(&self, desc: &RenderPipelineDescriptor<'_>) -> RenderPipeline {
        require(
            desc.multiview_mask.is_none() && desc.cache.is_none(),
            "Unsupported pipeline configuration",
        );
        let layout = desc
            .layout
            .cloned()
            .unwrap_or(PipelineLayout { group: None });
        let fragment = desc.fragment.as_ref().expect("Fragment shader required");
        require(
            fragment.targets.len() == 1,
            "Exactly one color target required",
        );
        let color = fragment.targets[0].expect("Color target required");
        require(
            color.write_mask == ColorWrites::ALL,
            "Partial color write masks unsupported",
        );
        if let Some(depth) = desc.depth_stencil {
            require(
                depth.format == TextureFormat::Depth32Float,
                "Unsupported depth format",
            );
        }
        require(
            desc.vertex.buffers.len() <= 1,
            "One vertex buffer supported",
        );
        let vertex = desc.vertex.buffers.first().map(|b| {
            let b = b.as_ref().expect("Missing vertex layout");
            require(
                !b.attributes.is_empty()
                    && b.attributes.iter().all(|attribute| {
                        attribute.shader_location < 16
                            && attribute
                                .offset
                                .checked_add(attribute.format.size())
                                .is_some_and(|end| end <= b.array_stride)
                    })
                    && b.attributes.iter().enumerate().all(|(index, attribute)| {
                        b.attributes[..index]
                            .iter()
                            .all(|previous| previous.shader_location != attribute.shader_location)
                    }),
                "Unsupported vertex layout",
            );
            (b.array_stride, b.step_mode)
        });
        RenderPipeline {
            native: Rc::new(
                self.shared
                    .native
                    .pipeline(desc)
                    .expect("Create render pipeline"),
            ),
            layout,
            vertex,
            format: color.format,
            depth: desc.depth_stencil.is_some(),
            owner: Rc::downgrade(&self.shared),
        }
    }

    pub fn create_command_encoder(&self, _: &CommandEncoderDescriptor) -> CommandEncoder {
        CommandEncoder {
            owner: Rc::downgrade(&self.shared),
            passes: Vec::new(),
        }
    }
}

impl Shared {
    fn check(self: &Rc<Self>, owner: &Weak<Self>) {
        require(
            Weak::ptr_eq(&Rc::downgrade(self), owner),
            "Resource belongs to another device",
        );
    }
}

enum Upload {
    Buffer {
        buffer: Buffer,
        offset: u64,
        data: Vec<u8>,
    },
    Texture {
        texture: Texture,
        mip: u32,
        origin: Origin3d,
        size: Extent3d,
        data: Vec<u8>,
    },
}

impl Queue {
    pub fn write_buffer(&self, buffer: &Buffer, offset: u64, data: &[u8]) {
        self.shared.check(&buffer.inner.owner);
        require(
            buffer.inner.usage.contains(BufferUsages::COPY_DST) && !buffer.inner.mapped.get(),
            "Buffer cannot be written",
        );
        require(
            offset.is_multiple_of(4) && data.len().is_multiple_of(4),
            "Buffer writes require four-byte alignment",
        );
        require(
            offset <= buffer.inner.size && data.len() as u64 <= buffer.inner.size - offset,
            "Buffer write out of bounds",
        );
        if !data.is_empty() {
            self.shared.uploads.borrow_mut().push(Upload::Buffer {
                buffer: buffer.clone(),
                offset,
                data: data.into(),
            });
        }
    }

    pub fn write_texture(
        &self,
        dest: TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: TexelCopyBufferLayout,
        size: Extent3d,
    ) {
        self.shared.check(&dest.texture.inner.owner);
        let texture = &dest.texture.inner;
        require(
            texture.usage.contains(TextureUsages::COPY_DST)
                && texture.format != TextureFormat::Depth32Float,
            "Texture cannot be written",
        );
        require(dest.mip_level < texture.mips, "Mip level out of bounds");
        require(
            size.width > 0 && size.height > 0 && size.depth_or_array_layers > 0,
            "Empty texture write",
        );
        require(
            u64::from(dest.origin.x) + u64::from(size.width)
                <= u64::from((texture.size.width >> dest.mip_level).max(1))
                && u64::from(dest.origin.y) + u64::from(size.height)
                    <= u64::from((texture.size.height >> dest.mip_level).max(1))
                && u64::from(dest.origin.z) + u64::from(size.depth_or_array_layers)
                    <= u64::from(texture.size.depth_or_array_layers),
            "Texture write out of bounds",
        );
        let packed = pack_texture(data, &layout, size);
        self.shared.uploads.borrow_mut().push(Upload::Texture {
            texture: dest.texture.clone(),
            mip: dest.mip_level,
            origin: dest.origin,
            size,
            data: packed,
        });
    }

    pub fn submit(&self, commands: impl IntoIterator<Item = CommandBuffer>) {
        let mut passes = Vec::new();
        for command in commands {
            self.shared.check(&command.owner);
            passes.extend(command.passes);
        }
        let uploads = std::mem::take(&mut *self.shared.uploads.borrow_mut());
        self.shared
            .native
            .submit(uploads, passes)
            .expect("Submit graphics commands");
    }
}

fn pack_texture(data: &[u8], layout: &TexelCopyBufferLayout, size: Extent3d) -> Vec<u8> {
    let row = usize::try_from(u64::from(size.width) * 4).expect("Texture row overflow");
    let pitch = layout.bytes_per_row.map_or(row, |n| n as usize);
    let rows = layout.rows_per_image.unwrap_or(size.height) as usize;
    require(
        pitch >= row && rows >= size.height as usize,
        "Invalid texture upload stride",
    );
    let total = row
        .checked_mul(size.height as usize)
        .and_then(|n| n.checked_mul(size.depth_or_array_layers as usize))
        .expect("Texture upload overflow");
    let mut packed = Vec::with_capacity(total);
    let offset = usize::try_from(layout.offset).expect("Texture offset overflow");
    for layer in 0..size.depth_or_array_layers as usize {
        for y in 0..size.height as usize {
            let start = layer
                .checked_mul(rows)
                .and_then(|n| n.checked_add(y))
                .and_then(|n| n.checked_mul(pitch))
                .and_then(|n| n.checked_add(offset))
                .expect("Texture offset overflow");
            let end = start.checked_add(row).expect("Texture offset overflow");
            packed.extend_from_slice(data.get(start..end).expect("Texture upload data too short"));
        }
    }
    packed
}

pub struct CommandEncoder {
    owner: Weak<Shared>,
    passes: Vec<Pass>,
}

pub struct CommandBuffer {
    owner: Weak<Shared>,
    passes: Vec<Pass>,
}

struct Pass {
    color: TextureView,
    depth: Option<TextureView>,
    clear: Option<Color>,
    clear_depth: Option<f32>,
    store_depth: bool,
    draws: Vec<Draw>,
}

struct Draw {
    pipeline: RenderPipeline,
    group: Option<BindGroup>,
    vertex: Option<(Buffer, Range<u64>)>,
    vertices: Range<u32>,
    instances: Range<u32>,
    viewport: [f32; 6],
    scissor: [u32; 4],
}

pub struct RenderPass<'a> {
    encoder: &'a mut CommandEncoder,
    pass: Pass,
    pipeline: Option<RenderPipeline>,
    group: Option<BindGroup>,
    vertex: Option<(Buffer, Range<u64>)>,
    viewport: [f32; 6],
    scissor: [u32; 4],
}

impl CommandEncoder {
    pub fn begin_render_pass(&mut self, desc: &RenderPassDescriptor<'_>) -> RenderPass<'_> {
        require(
            desc.color_attachments.len() == 1,
            "One color attachment required",
        );
        let color = desc.color_attachments[0]
            .as_ref()
            .expect("Missing color attachment");
        require(
            color.resolve_target.is_none()
                && color.depth_slice.is_none()
                && desc
                    .depth_stencil_attachment
                    .as_ref()
                    .is_none_or(|depth| depth.stencil_ops.is_none()),
            "Unsupported attachment configuration",
        );
        let owner = self.owner.upgrade().expect("Device dropped");
        owner.check(&color.view.texture.inner.owner);
        if let Some(depth) = &desc.depth_stencil_attachment {
            owner.check(&depth.view.texture.inner.owner);
        }
        for view in std::iter::once(color.view).chain(
            desc.depth_stencil_attachment
                .as_ref()
                .map(|depth| depth.view),
        ) {
            require(
                view.dimension == TextureViewDimension::D2
                    && view.texture.inner.size.depth_or_array_layers == 1
                    && view.texture.inner.mips == 1,
                "Attachments require a single-layer, single-mip D2 view",
            );
        }
        require(
            color.view.texture.inner.format != TextureFormat::Depth32Float,
            "Depth format used as color attachment",
        );
        let size = color.view.texture.inner.size;
        if let Some(depth) = &desc.depth_stencil_attachment {
            require(
                depth.view.texture.inner.size.width == size.width
                    && depth.view.texture.inner.size.height == size.height
                    && depth.view.texture.inner.format == TextureFormat::Depth32Float,
                "Depth attachment mismatch",
            );
        }
        require(
            color
                .view
                .texture
                .inner
                .usage
                .contains(TextureUsages::RENDER_ATTACHMENT)
                && desc.depth_stencil_attachment.as_ref().is_none_or(|depth| {
                    depth
                        .view
                        .texture
                        .inner
                        .usage
                        .contains(TextureUsages::RENDER_ATTACHMENT)
                }),
            "Attachment usage mismatch",
        );
        require(
            matches!(color.ops.store, StoreOp::Store),
            "Color discard unsupported",
        );
        let clear = match color.ops.load {
            LoadOp::Load => None,
            LoadOp::Clear(value) => Some(value),
        };
        let depth_ops = desc
            .depth_stencil_attachment
            .as_ref()
            .map(|depth| depth.depth_ops.expect("Depth operations required"));
        let clear_depth = depth_ops.and_then(|ops| match ops.load {
            LoadOp::Load => None,
            LoadOp::Clear(value) => {
                require(
                    value.is_finite() && (0. ..=1.).contains(&value),
                    "Invalid depth clear value",
                );
                Some(value)
            }
        });
        RenderPass {
            encoder: self,
            pass: Pass {
                color: color.view.clone(),
                depth: desc
                    .depth_stencil_attachment
                    .as_ref()
                    .map(|depth| depth.view.clone()),
                clear,
                clear_depth,
                store_depth: depth_ops.is_some_and(|ops| matches!(ops.store, StoreOp::Store)),
                draws: Vec::new(),
            },
            pipeline: None,
            group: None,
            vertex: None,
            viewport: [0., 0., size.width as f32, size.height as f32, 0., 1.],
            scissor: [0, 0, size.width, size.height],
        }
    }

    pub fn finish(self) -> CommandBuffer {
        CommandBuffer {
            owner: self.owner,
            passes: self.passes,
        }
    }
}

impl RenderPass<'_> {
    pub fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        self.encoder
            .owner
            .upgrade()
            .expect("device has been dropped")
            .check(&pipeline.owner);
        self.pipeline = Some(pipeline.clone());
    }

    pub fn set_bind_group(&mut self, index: u32, group: &BindGroup, offsets: &[u32]) {
        require(
            index == 0 && offsets.is_empty(),
            "Only group zero without dynamic offsets supported",
        );
        self.encoder
            .owner
            .upgrade()
            .expect("device has been dropped")
            .check(&group.owner);
        for (_, resource) in group.entries.iter() {
            if let Resource::Texture(view) = resource {
                require(
                    !Rc::ptr_eq(&view.texture.inner, &self.pass.color.texture.inner)
                        && self.pass.depth.as_ref().is_none_or(|depth| {
                            !Rc::ptr_eq(&view.texture.inner, &depth.texture.inner)
                        }),
                    "Cannot sample a render attachment in the same pass",
                );
            }
        }
        self.group = Some(group.clone());
    }

    pub fn set_vertex_buffer(&mut self, slot: u32, slice: BufferSlice<'_>) {
        require(
            slot == 0
                && slice.buffer.inner.usage.contains(BufferUsages::VERTEX)
                && !slice.buffer.inner.mapped.get(),
            "Invalid vertex buffer",
        );
        self.encoder
            .owner
            .upgrade()
            .expect("device has been dropped")
            .check(&slice.buffer.inner.owner);
        self.vertex = Some((slice.buffer.clone(), slice.range));
    }

    pub fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        require(
            [x, y, width, height, min_depth, max_depth]
                .iter()
                .all(|n| n.is_finite())
                && x >= 0.
                && y >= 0.
                && width > 0.
                && height > 0.
                && min_depth >= 0.
                && max_depth <= 1.
                && min_depth <= max_depth,
            "Invalid viewport",
        );
        self.viewport = [x, y, width, height, min_depth, max_depth];
    }

    pub fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        let size = self.pass.color.texture.inner.size;
        require(
            u64::from(x) + u64::from(width) <= u64::from(size.width)
                && u64::from(y) + u64::from(height) <= u64::from(size.height),
            "Scissor out of bounds",
        );
        self.scissor = [x, y, width, height];
    }

    pub fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        if vertices.is_empty()
            || instances.is_empty()
            || self.scissor[2] == 0
            || self.scissor[3] == 0
        {
            return;
        }
        let pipeline = self.pipeline.as_ref().expect("Pipeline not set");
        require(
            pipeline.format == self.pass.color.texture.inner.format
                && pipeline.depth == self.pass.depth.is_some()
                && match (&pipeline.layout.group, &self.group) {
                    (None, None) => true,
                    (Some(layout), Some(group)) => layout.entries == group.layout.entries,
                    _ => false,
                },
            "Pipeline compatibility mismatch",
        );
        if let Some(group) = &self.group {
            for (_, resource) in group.entries.iter() {
                if let Resource::Buffer(buffer) = resource {
                    require(!buffer.inner.mapped.get(), "Bound buffer is mapped");
                }
            }
        }
        if let Some((stride, step_mode)) = pipeline.vertex {
            let (_, range) = self.vertex.as_ref().expect("Vertex buffer not set");
            let end = match step_mode {
                VertexStepMode::Vertex => vertices.end,
                VertexStepMode::Instance => instances.end,
            };
            require(
                u64::from(end) * stride <= range.end - range.start,
                "Vertex data out of bounds",
            );
        }
        self.pass.draws.push(Draw {
            pipeline: pipeline.clone(),
            group: self.group.clone(),
            vertex: self.vertex.clone(),
            vertices,
            instances,
            viewport: self.viewport,
            scissor: self.scissor,
        });
    }
}

impl Drop for RenderPass<'_> {
    fn drop(&mut self) {
        let replacement = Pass {
            color: self.pass.color.clone(),
            depth: self.pass.depth.clone(),
            clear: self.pass.clear,
            clear_depth: self.pass.clear_depth,
            store_depth: self.pass.store_depth,
            draws: Vec::new(),
        };
        self.encoder
            .passes
            .push(std::mem::replace(&mut self.pass, replacement));
    }
}

#[macro_export]
macro_rules! include_wgsl {
    ($path:literal) => {
        include!(concat!(env!("OUT_DIR"), "/", $path, ".rs"))
    };
}

#[macro_export]
macro_rules! vertex_attr_array {
    ($location:literal => $format:ident) => {
        [$crate::VertexAttribute {
            format: $crate::VertexFormat::$format,
            offset: 0,
            shader_location: $location,
        }]
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "macos", feature = "image"))]
    #[test]
    fn headless_bitmap_readback() {
        let instance = Instance::default();
        let adapter = instance.request_adapter(&Default::default()).unwrap();
        let (device, queue) = adapter.request_device(&Default::default()).unwrap();
        let target = device.create_bitmap_target(2, 2);
        let view = target.create_view();
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color {
                            r: 1.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
            });
        }
        queue.submit([encoder.finish()]);
        let bitmap = target.read(&queue).unwrap();
        assert_eq!(bitmap.data(), [255, 0, 0, 255].repeat(4));
    }

    #[test]
    fn native_shader_binaries_are_selected_by_entry_point() {
        let shared = ShaderModule {
            source: NativeSource::Binary(vec![(String::new(), vec![1, 2])]),
            entries: vec![("vertex".into(), "vs".into())],
        };
        assert_eq!(shared.binary(Some("vertex")), Some([1, 2].as_slice()));

        let separate = ShaderModule {
            source: NativeSource::Binary(vec![("vs".into(), vec![3]), ("fs".into(), vec![4])]),
            entries: vec![
                ("vertex".into(), "vs".into()),
                ("fragment".into(), "fs".into()),
            ],
        };
        assert_eq!(separate.binary(Some("vertex")), Some([3].as_slice()));
        assert_eq!(separate.binary(Some("fragment")), Some([4].as_slice()));

        #[cfg(target_os = "windows")]
        {
            let backends = ShaderModule {
                source: NativeSource::Binary(vec![
                    ("dx12:vs".into(), vec![7]),
                    ("dx12:fs".into(), vec![8]),
                ]),
                entries: vec![
                    ("vertex".into(), "vs".into()),
                    ("fragment".into(), "fs".into()),
                ],
            };
            assert_eq!(
                backends.binary_for("dx12", Some("vertex")),
                Some([7].as_slice())
            );
            assert_eq!(
                backends.binary_for("dx12", Some("fragment")),
                Some([8].as_slice())
            );
        }

        let exact_over_shared = ShaderModule {
            source: NativeSource::Binary(vec![(String::new(), vec![5]), ("vs".into(), vec![6])]),
            entries: vec![("vertex".into(), "vs".into())],
        };
        assert_eq!(
            exact_over_shared.binary(Some("vertex")),
            Some([6].as_slice())
        );
    }

    #[test]
    fn padded_texture_rows_and_layers() {
        let bytes: Vec<u8> = (0..64).collect();
        let result = pack_texture(
            &bytes,
            &TexelCopyBufferLayout {
                offset: 4,
                bytes_per_row: Some(8),
                rows_per_image: Some(3),
            },
            Extent3d {
                width: 1,
                height: 2,
                depth_or_array_layers: 2,
            },
        );
        assert_eq!(
            result,
            [4, 5, 6, 7, 12, 13, 14, 15, 28, 29, 30, 31, 36, 37, 38, 39]
        );
    }

    #[test]
    #[should_panic(expected = "too short")]
    fn short_texture_data_rejected() {
        pack_texture(
            &[0; 3],
            &TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: None,
                rows_per_image: None,
            },
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    #[test]
    fn mapped_view_slice_access_stays_within_mapping() {
        let data = RefCell::new(vec![1, 2, 3, 4, 5, 6]);
        {
            let mut view = BufferViewMut {
                data: data.borrow_mut(),
                range: 2..5,
            };
            assert_eq!(&*view, &[3, 4, 5]);
            view.copy_from_slice(&[7, 8, 9]);
            view[1] = 10;
            assert_eq!(&*view, &[7, 10, 9]);
        }
        assert_eq!(*data.borrow(), [1, 2, 7, 10, 9, 6]);
    }

    #[test]
    fn buffer_ranges() {
        assert_eq!(bounds(.., 12), 0..12);
        assert_eq!(bounds(4..=7, 12), 4..8);
        assert_eq!(bounds(12.., 12), 12..12);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn invalid_range_rejected() {
        bounds(4..13, 12);
    }
}
