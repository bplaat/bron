/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::*;

bitflags::bitflags! {
    /// Supported buffer usages.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct BufferUsages: u32 {
        const COPY_DST = 1;
        const VERTEX = 2;
        const UNIFORM = 4;
        const STORAGE = 8;
    }

    /// Supported texture usages.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct TextureUsages: u32 {
        const COPY_DST = 1;
        const TEXTURE_BINDING = 2;
        const RENDER_ATTACHMENT = 4;
        const COPY_SRC = 8;
    }

    /// Shader-stage visibility.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ShaderStages: u32 {
        const VERTEX = 1;
        const FRAGMENT = 2;
        const VERTEX_FRAGMENT = Self::VERTEX.bits() | Self::FRAGMENT.bits();
    }

    /// Color-channel write mask.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct ColorWrites: u32 {
        const ALL = 15;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFormat {
    Rgba8UnormSrgb,
    Bgra8UnormSrgb,
    Bgra8Unorm,
    Rgba8Unorm,
    Depth32Float,
}

impl TextureFormat {
    pub const fn is_srgb(&self) -> bool {
        matches!(self, Self::Rgba8UnormSrgb | Self::Bgra8UnormSrgb)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureDimension {
    D2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextureViewDimension {
    #[default]
    D2,
    D2Array,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureAspect {
    All,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AddressMode {
    #[default]
    ClampToEdge,
    Repeat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterMode {
    #[default]
    Nearest,
    Linear,
}

pub type MipmapFilterMode = FilterMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareFunction {
    Always,
    LessEqual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexStepMode {
    Vertex,
    Instance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexFormat {
    Float32x2,
    Float32x3,
    Uint32x2,
}

impl VertexFormat {
    pub const fn size(self) -> u64 {
        match self {
            Self::Float32x2 => 8,
            Self::Float32x3 => 12,
            Self::Uint32x2 => 8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VertexAttribute {
    pub format: VertexFormat,
    pub offset: u64,
    pub shader_location: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct VertexBufferLayout<'a> {
    pub array_stride: u64,
    pub step_mode: VertexStepMode,
    pub attributes: &'a [VertexAttribute],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Extent3d {
    pub width: u32,
    pub height: u32,
    pub depth_or_array_layers: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Origin3d {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[derive(Default)]
pub struct RequestAdapterOptions<'a> {
    #[cfg(feature = "window")]
    pub compatible_surface: Option<&'a Surface<'a>>,
    #[cfg(not(feature = "window"))]
    _lifetime: std::marker::PhantomData<&'a ()>,
}

#[derive(Default)]
pub struct DeviceDescriptor {
    pub label: Option<&'static str>,
}

pub struct BufferDescriptor<'a> {
    pub label: Option<&'a str>,
    pub size: u64,
    pub usage: BufferUsages,
    pub mapped_at_creation: bool,
}

pub struct TextureDescriptor<'a> {
    pub label: Option<&'a str>,
    pub size: Extent3d,
    pub mip_level_count: u32,
    pub sample_count: u32,
    pub dimension: TextureDimension,
    pub format: TextureFormat,
    pub usage: TextureUsages,
    pub view_formats: &'a [TextureFormat],
}

#[derive(Default)]
pub struct TextureViewDescriptor {
    pub dimension: Option<TextureViewDimension>,
}

#[derive(Clone, Copy, Default)]
pub struct SamplerDescriptor<'a> {
    pub label: Option<&'a str>,
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_filter: MipmapFilterMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferBindingType {
    Uniform,
    Storage { read_only: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureSampleType {
    Float { filterable: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SamplerBindingType {
    Filtering,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingType {
    Buffer {
        ty: BufferBindingType,
        has_dynamic_offset: bool,
        min_binding_size: Option<std::num::NonZeroU64>,
    },
    Texture {
        sample_type: TextureSampleType,
        view_dimension: TextureViewDimension,
        multisampled: bool,
    },
    Sampler(SamplerBindingType),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindGroupLayoutEntry {
    pub binding: u32,
    pub visibility: ShaderStages,
    pub ty: BindingType,
    pub count: Option<std::num::NonZeroU32>,
}

pub struct BindGroupLayoutDescriptor<'a> {
    pub label: Option<&'a str>,
    pub entries: &'a [BindGroupLayoutEntry],
}

pub struct BufferBinding<'a> {
    pub buffer: &'a Buffer,
}

pub enum BindingResource<'a> {
    Buffer(BufferBinding<'a>),
    TextureView(&'a TextureView),
    Sampler(&'a Sampler),
}

pub struct BindGroupEntry<'a> {
    pub binding: u32,
    pub resource: BindingResource<'a>,
}

pub struct BindGroupDescriptor<'a> {
    pub label: Option<&'a str>,
    pub layout: &'a BindGroupLayout,
    pub entries: &'a [BindGroupEntry<'a>],
}

#[derive(Default)]
pub struct PipelineLayoutDescriptor<'a> {
    pub label: Option<&'a str>,
    pub bind_group_layouts: &'a [Option<&'a BindGroupLayout>],
}

#[derive(Clone, Copy)]
pub enum ShaderSource<'a> {
    /// Metal Shading Language source for runtime compilation on macOS.
    Text(&'a str),
    /// Platform-native binaries keyed by native entry point.
    /// An empty entry-point name denotes one library shared by all entries.
    Binary(&'a [(&'a str, &'a [u8])]),
    /// SPIR-V words for Vulkan.
    SpirV(&'a [u32]),
}

#[derive(Clone, Copy)]
pub struct ShaderModuleDescriptor<'a> {
    pub label: Option<&'a str>,
    pub source: ShaderSource<'a>,
    pub entries: &'a [(&'a str, &'a str)],
}

#[derive(Default)]
pub struct PipelineCompilationOptions {}

pub struct VertexState<'a> {
    pub module: &'a ShaderModule,
    pub entry_point: Option<&'a str>,
    pub compilation_options: PipelineCompilationOptions,
    pub buffers: &'a [Option<VertexBufferLayout<'a>>],
}

pub struct FragmentState<'a> {
    pub module: &'a ShaderModule,
    pub entry_point: Option<&'a str>,
    pub compilation_options: PipelineCompilationOptions,
    pub targets: &'a [Option<ColorTargetState>],
}

#[derive(Clone, Copy, Default)]
pub struct PrimitiveState {
    pub cull_mode: Option<Face>,
}

#[derive(Clone, Copy)]
pub struct BlendState;

impl BlendState {
    pub const ALPHA_BLENDING: Self = Self;
}

#[derive(Clone, Copy)]
pub struct ColorTargetState {
    pub format: TextureFormat,
    pub blend: Option<BlendState>,
    pub write_mask: ColorWrites,
}

#[derive(Clone, Copy, Default)]
pub struct StencilState {}

#[derive(Clone, Copy, Default)]
pub struct DepthBiasState {}

#[derive(Clone, Copy)]
pub struct DepthStencilState {
    pub format: TextureFormat,
    pub depth_write_enabled: Option<bool>,
    pub depth_compare: Option<CompareFunction>,
    pub stencil: StencilState,
    pub bias: DepthBiasState,
}

#[derive(Default)]
pub struct MultisampleState {}

pub struct RenderPipelineDescriptor<'a> {
    pub label: Option<&'a str>,
    pub layout: Option<&'a PipelineLayout>,
    pub vertex: VertexState<'a>,
    pub primitive: PrimitiveState,
    pub depth_stencil: Option<DepthStencilState>,
    pub multisample: MultisampleState,
    pub fragment: Option<FragmentState<'a>>,
    pub multiview_mask: Option<std::num::NonZeroU32>,
    pub cache: Option<()>,
}

#[derive(Clone, Copy)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0.,
        g: 0.,
        b: 0.,
        a: 1.,
    };

    /// Convert a packed `0xRRGGBB` sRGB color to a clear color for the target format.
    pub fn from_rgb8(color: u32, format: TextureFormat) -> Self {
        fn channel(value: u8, srgb: bool) -> f64 {
            let value = f64::from(value) / 255.0;
            if !srgb {
                value
            } else if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }

        let srgb = format.is_srgb();
        Self {
            r: channel((color >> 16) as u8, srgb),
            g: channel((color >> 8) as u8, srgb),
            b: channel(color as u8, srgb),
            a: 1.0,
        }
    }
}

#[cfg(test)]
mod color_tests {
    use super::{Color, TextureFormat};

    #[test]
    fn rgb8_clear_color_matches_target_encoding() {
        let unorm = Color::from_rgb8(0x808080, TextureFormat::Bgra8Unorm);
        let srgb = Color::from_rgb8(0x808080, TextureFormat::Bgra8UnormSrgb);
        let navy = Color::from_rgb8(0x04050a, TextureFormat::Bgra8Unorm);

        assert!((unorm.r - 128.0 / 255.0).abs() < 1e-12);
        assert!((srgb.r - 0.215_860_500_113_899_26).abs() < 1e-12);
        assert!((navy.r - 4.0 / 255.0).abs() < 1e-12);
        assert!((navy.g - 5.0 / 255.0).abs() < 1e-12);
        assert!((navy.b - 10.0 / 255.0).abs() < 1e-12);
        assert_eq!(unorm.a, 1.0);
        assert_eq!(srgb.a, 1.0);
    }
}

#[derive(Clone, Copy)]
pub enum LoadOp<T> {
    Load,
    Clear(T),
}

#[derive(Clone, Copy)]
pub enum StoreOp {
    Store,
    Discard,
}

#[derive(Clone, Copy)]
pub struct Operations<T> {
    pub load: LoadOp<T>,
    pub store: StoreOp,
}

pub struct RenderPassColorAttachment<'a> {
    pub view: &'a TextureView,
    pub depth_slice: Option<u32>,
    pub resolve_target: Option<&'a TextureView>,
    pub ops: Operations<Color>,
}

pub struct RenderPassDepthStencilAttachment<'a> {
    pub view: &'a TextureView,
    pub depth_ops: Option<Operations<f32>>,
    pub stencil_ops: Option<Operations<u32>>,
}

#[derive(Default)]
pub struct RenderPassDescriptor<'a> {
    pub label: Option<&'a str>,
    pub color_attachments: &'a [Option<RenderPassColorAttachment<'a>>],
    pub depth_stencil_attachment: Option<RenderPassDepthStencilAttachment<'a>>,
}

#[derive(Default)]
pub struct CommandEncoderDescriptor {
    pub label: Option<&'static str>,
}

pub struct TexelCopyTextureInfo<'a> {
    pub texture: &'a Texture,
    pub mip_level: u32,
    pub origin: Origin3d,
    pub aspect: TextureAspect,
}

pub struct TexelCopyBufferLayout {
    pub offset: u64,
    pub bytes_per_row: Option<u32>,
    pub rows_per_image: Option<u32>,
}
