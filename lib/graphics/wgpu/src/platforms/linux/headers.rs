/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Vulkan ABI subset, derived from Vulkan-Headers via ash 0.38 (MIT OR Apache-2.0).
// No generated binding crate is linked. All declarations are C POD; zero defaults
// initialize optional pointers and reserved fields before required fields are set.
#![allow(non_camel_case_types, dead_code)]
#![allow(unreachable_pub)]
use std::ffi::{c_char, c_ulong, c_void};
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InstanceCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: InstanceCreateFlags,
    pub p_application_info: *const ApplicationInfo,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
}
impl Default for InstanceCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type StructureType = u32;
pub type InstanceCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ApplicationInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub p_application_name: *const c_char,
    pub application_version: u32,
    pub p_engine_name: *const c_char,
    pub engine_version: u32,
    pub api_version: u32,
}
impl Default for ApplicationInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type AllocationCallbacks = c_void;
pub type Instance = *mut c_void;
pub type PhysicalDevice = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueueFamilyProperties {
    pub queue_flags: QueueFlags,
    pub queue_count: u32,
    pub timestamp_valid_bits: u32,
    pub min_image_transfer_granularity: Extent3D,
}
impl Default for QueueFamilyProperties {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type QueueFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Extent3D {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}
impl Default for Extent3D {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicalDeviceMemoryProperties {
    pub memory_type_count: u32,
    pub memory_types: [MemoryType; 32],
    pub memory_heap_count: u32,
    pub memory_heaps: [MemoryHeap; 16],
}
impl Default for PhysicalDeviceMemoryProperties {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryType {
    pub property_flags: MemoryPropertyFlags,
    pub heap_index: u32,
}
impl Default for MemoryType {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type MemoryPropertyFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryHeap {
    pub size: DeviceSize,
    pub flags: MemoryHeapFlags,
}
impl Default for MemoryHeap {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DeviceSize = u64;
pub type MemoryHeapFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeviceCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: DeviceCreateFlags,
    pub queue_create_info_count: u32,
    pub p_queue_create_infos: *const DeviceQueueCreateInfo,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
    pub p_enabled_features: *const PhysicalDeviceFeatures,
}
impl Default for DeviceCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DeviceCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeviceQueueCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: DeviceQueueCreateFlags,
    pub queue_family_index: u32,
    pub queue_count: u32,
    pub p_queue_priorities: *const f32,
}
impl Default for DeviceQueueCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DeviceQueueCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PhysicalDeviceFeatures {
    pub robust_buffer_access: Bool32,
    pub full_draw_index_uint32: Bool32,
    pub image_cube_array: Bool32,
    pub independent_blend: Bool32,
    pub geometry_shader: Bool32,
    pub tessellation_shader: Bool32,
    pub sample_rate_shading: Bool32,
    pub dual_src_blend: Bool32,
    pub logic_op: Bool32,
    pub multi_draw_indirect: Bool32,
    pub draw_indirect_first_instance: Bool32,
    pub depth_clamp: Bool32,
    pub depth_bias_clamp: Bool32,
    pub fill_mode_non_solid: Bool32,
    pub depth_bounds: Bool32,
    pub wide_lines: Bool32,
    pub large_points: Bool32,
    pub alpha_to_one: Bool32,
    pub multi_viewport: Bool32,
    pub sampler_anisotropy: Bool32,
    pub texture_compression_etc2: Bool32,
    pub texture_compression_astc_ldr: Bool32,
    pub texture_compression_bc: Bool32,
    pub occlusion_query_precise: Bool32,
    pub pipeline_statistics_query: Bool32,
    pub vertex_pipeline_stores_and_atomics: Bool32,
    pub fragment_stores_and_atomics: Bool32,
    pub shader_tessellation_and_geometry_point_size: Bool32,
    pub shader_image_gather_extended: Bool32,
    pub shader_storage_image_extended_formats: Bool32,
    pub shader_storage_image_multisample: Bool32,
    pub shader_storage_image_read_without_format: Bool32,
    pub shader_storage_image_write_without_format: Bool32,
    pub shader_uniform_buffer_array_dynamic_indexing: Bool32,
    pub shader_sampled_image_array_dynamic_indexing: Bool32,
    pub shader_storage_buffer_array_dynamic_indexing: Bool32,
    pub shader_storage_image_array_dynamic_indexing: Bool32,
    pub shader_clip_distance: Bool32,
    pub shader_cull_distance: Bool32,
    pub shader_float64: Bool32,
    pub shader_int64: Bool32,
    pub shader_int16: Bool32,
    pub shader_resource_residency: Bool32,
    pub shader_resource_min_lod: Bool32,
    pub sparse_binding: Bool32,
    pub sparse_residency_buffer: Bool32,
    pub sparse_residency_image2_d: Bool32,
    pub sparse_residency_image3_d: Bool32,
    pub sparse_residency2_samples: Bool32,
    pub sparse_residency4_samples: Bool32,
    pub sparse_residency8_samples: Bool32,
    pub sparse_residency16_samples: Bool32,
    pub sparse_residency_aliased: Bool32,
    pub variable_multisample_rate: Bool32,
    pub inherited_queries: Bool32,
}
impl Default for PhysicalDeviceFeatures {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type Bool32 = u32;
pub type Device = *mut c_void;
pub type Queue = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XlibSurfaceCreateInfoKHR {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: XlibSurfaceCreateFlagsKHR,
    pub dpy: *mut Display,
    pub window: Window,
}
impl Default for XlibSurfaceCreateInfoKHR {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type XlibSurfaceCreateFlagsKHR = u32;
pub type Display = c_void;
pub type Window = c_ulong;
pub type SurfaceKHR = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WaylandSurfaceCreateInfoKHR {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: WaylandSurfaceCreateFlagsKHR,
    pub display: *mut wl_display,
    pub surface: *mut wl_surface,
}
impl Default for WaylandSurfaceCreateInfoKHR {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type WaylandSurfaceCreateFlagsKHR = u32;
pub type wl_display = c_void;
pub type wl_surface = c_void;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SurfaceFormatKHR {
    pub format: Format,
    pub color_space: ColorSpaceKHR,
}
impl Default for SurfaceFormatKHR {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type Format = u32;
pub type ColorSpaceKHR = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SurfaceCapabilitiesKHR {
    pub min_image_count: u32,
    pub max_image_count: u32,
    pub current_extent: Extent2D,
    pub min_image_extent: Extent2D,
    pub max_image_extent: Extent2D,
    pub max_image_array_layers: u32,
    pub supported_transforms: SurfaceTransformFlagsKHR,
    pub current_transform: SurfaceTransformFlagsKHR,
    pub supported_composite_alpha: CompositeAlphaFlagsKHR,
    pub supported_usage_flags: ImageUsageFlags,
}
impl Default for SurfaceCapabilitiesKHR {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}
impl Default for Extent2D {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type SurfaceTransformFlagsKHR = u32;
pub type CompositeAlphaFlagsKHR = u32;
pub type ImageUsageFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SwapchainCreateInfoKHR {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: SwapchainCreateFlagsKHR,
    pub surface: SurfaceKHR,
    pub min_image_count: u32,
    pub image_format: Format,
    pub image_color_space: ColorSpaceKHR,
    pub image_extent: Extent2D,
    pub image_array_layers: u32,
    pub image_usage: ImageUsageFlags,
    pub image_sharing_mode: SharingMode,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
    pub pre_transform: SurfaceTransformFlagsKHR,
    pub composite_alpha: CompositeAlphaFlagsKHR,
    pub present_mode: PresentModeKHR,
    pub clipped: Bool32,
    pub old_swapchain: SwapchainKHR,
}
impl Default for SwapchainCreateInfoKHR {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type SwapchainCreateFlagsKHR = u32;
pub type SharingMode = u32;
pub type PresentModeKHR = u32;
pub type SwapchainKHR = u64;
pub type Image = u64;
pub type Semaphore = u64;
pub type Fence = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PresentInfoKHR {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub p_wait_semaphores: *const Semaphore,
    pub swapchain_count: u32,
    pub p_swapchains: *const SwapchainKHR,
    pub p_image_indices: *const u32,
    pub p_results: *mut Result,
}
impl Default for PresentInfoKHR {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type Result = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BufferCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: BufferCreateFlags,
    pub size: DeviceSize,
    pub usage: BufferUsageFlags,
    pub sharing_mode: SharingMode,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
}
impl Default for BufferCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type BufferCreateFlags = u32;
pub type BufferUsageFlags = u32;
pub type Buffer = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryRequirements {
    pub size: DeviceSize,
    pub alignment: DeviceSize,
    pub memory_type_bits: u32,
}
impl Default for MemoryRequirements {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryAllocateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub allocation_size: DeviceSize,
    pub memory_type_index: u32,
}
impl Default for MemoryAllocateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DeviceMemory = u64;
pub type MemoryMapFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImageCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: ImageCreateFlags,
    pub image_type: ImageType,
    pub format: Format,
    pub extent: Extent3D,
    pub mip_levels: u32,
    pub array_layers: u32,
    pub samples: SampleCountFlags,
    pub tiling: ImageTiling,
    pub usage: ImageUsageFlags,
    pub sharing_mode: SharingMode,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
    pub initial_layout: ImageLayout,
}
impl Default for ImageCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type ImageCreateFlags = u32;
pub type ImageType = u32;
pub type SampleCountFlags = u32;
pub type ImageTiling = u32;
pub type ImageLayout = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImageViewCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: ImageViewCreateFlags,
    pub image: Image,
    pub view_type: ImageViewType,
    pub format: Format,
    pub components: ComponentMapping,
    pub subresource_range: ImageSubresourceRange,
}
impl Default for ImageViewCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type ImageViewCreateFlags = u32;
pub type ImageViewType = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ComponentMapping {
    pub r: ComponentSwizzle,
    pub g: ComponentSwizzle,
    pub b: ComponentSwizzle,
    pub a: ComponentSwizzle,
}
impl Default for ComponentMapping {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type ComponentSwizzle = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImageSubresourceRange {
    pub aspect_mask: ImageAspectFlags,
    pub base_mip_level: u32,
    pub level_count: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}
impl Default for ImageSubresourceRange {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type ImageAspectFlags = u32;
pub type ImageView = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SamplerCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: SamplerCreateFlags,
    pub mag_filter: Filter,
    pub min_filter: Filter,
    pub mipmap_mode: SamplerMipmapMode,
    pub address_mode_u: SamplerAddressMode,
    pub address_mode_v: SamplerAddressMode,
    pub address_mode_w: SamplerAddressMode,
    pub mip_lod_bias: f32,
    pub anisotropy_enable: Bool32,
    pub max_anisotropy: f32,
    pub compare_enable: Bool32,
    pub compare_op: CompareOp,
    pub min_lod: f32,
    pub max_lod: f32,
    pub border_color: BorderColor,
    pub unnormalized_coordinates: Bool32,
}
impl Default for SamplerCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type SamplerCreateFlags = u32;
pub type Filter = u32;
pub type SamplerMipmapMode = u32;
pub type SamplerAddressMode = u32;
pub type CompareOp = u32;
pub type BorderColor = u32;
pub type Sampler = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShaderModuleCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: ShaderModuleCreateFlags,
    pub code_size: usize,
    pub p_code: *const u32,
}
impl Default for ShaderModuleCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type ShaderModuleCreateFlags = u32;
pub type ShaderModule = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorSetLayoutCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: DescriptorSetLayoutCreateFlags,
    pub binding_count: u32,
    pub p_bindings: *const DescriptorSetLayoutBinding,
}
impl Default for DescriptorSetLayoutCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DescriptorSetLayoutCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorSetLayoutBinding {
    pub binding: u32,
    pub descriptor_type: DescriptorType,
    pub descriptor_count: u32,
    pub stage_flags: ShaderStageFlags,
    pub p_immutable_samplers: *const Sampler,
}
impl Default for DescriptorSetLayoutBinding {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DescriptorType = u32;
pub type ShaderStageFlags = u32;
pub type DescriptorSetLayout = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineLayoutCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineLayoutCreateFlags,
    pub set_layout_count: u32,
    pub p_set_layouts: *const DescriptorSetLayout,
    pub push_constant_range_count: u32,
    pub p_push_constant_ranges: *const PushConstantRange,
}
impl Default for PipelineLayoutCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineLayoutCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PushConstantRange {
    pub stage_flags: ShaderStageFlags,
    pub offset: u32,
    pub size: u32,
}
impl Default for PushConstantRange {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineLayout = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RenderPassCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: RenderPassCreateFlags,
    pub attachment_count: u32,
    pub p_attachments: *const AttachmentDescription,
    pub subpass_count: u32,
    pub p_subpasses: *const SubpassDescription,
    pub dependency_count: u32,
    pub p_dependencies: *const SubpassDependency,
}
impl Default for RenderPassCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type RenderPassCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AttachmentDescription {
    pub flags: AttachmentDescriptionFlags,
    pub format: Format,
    pub samples: SampleCountFlags,
    pub load_op: AttachmentLoadOp,
    pub store_op: AttachmentStoreOp,
    pub stencil_load_op: AttachmentLoadOp,
    pub stencil_store_op: AttachmentStoreOp,
    pub initial_layout: ImageLayout,
    pub final_layout: ImageLayout,
}
impl Default for AttachmentDescription {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type AttachmentDescriptionFlags = u32;
pub type AttachmentLoadOp = u32;
pub type AttachmentStoreOp = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SubpassDescription {
    pub flags: SubpassDescriptionFlags,
    pub pipeline_bind_point: PipelineBindPoint,
    pub input_attachment_count: u32,
    pub p_input_attachments: *const AttachmentReference,
    pub color_attachment_count: u32,
    pub p_color_attachments: *const AttachmentReference,
    pub p_resolve_attachments: *const AttachmentReference,
    pub p_depth_stencil_attachment: *const AttachmentReference,
    pub preserve_attachment_count: u32,
    pub p_preserve_attachments: *const u32,
}
impl Default for SubpassDescription {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type SubpassDescriptionFlags = u32;
pub type PipelineBindPoint = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AttachmentReference {
    pub attachment: u32,
    pub layout: ImageLayout,
}
impl Default for AttachmentReference {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SubpassDependency {
    pub src_subpass: u32,
    pub dst_subpass: u32,
    pub src_stage_mask: PipelineStageFlags,
    pub dst_stage_mask: PipelineStageFlags,
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
    pub dependency_flags: DependencyFlags,
}
impl Default for SubpassDependency {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineStageFlags = u32;
pub type AccessFlags = u32;
pub type DependencyFlags = u32;
pub type RenderPass = u64;
pub type PipelineCache = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GraphicsPipelineCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineCreateFlags,
    pub stage_count: u32,
    pub p_stages: *const PipelineShaderStageCreateInfo,
    pub p_vertex_input_state: *const PipelineVertexInputStateCreateInfo,
    pub p_input_assembly_state: *const PipelineInputAssemblyStateCreateInfo,
    pub p_tessellation_state: *const PipelineTessellationStateCreateInfo,
    pub p_viewport_state: *const PipelineViewportStateCreateInfo,
    pub p_rasterization_state: *const PipelineRasterizationStateCreateInfo,
    pub p_multisample_state: *const PipelineMultisampleStateCreateInfo,
    pub p_depth_stencil_state: *const PipelineDepthStencilStateCreateInfo,
    pub p_color_blend_state: *const PipelineColorBlendStateCreateInfo,
    pub p_dynamic_state: *const PipelineDynamicStateCreateInfo,
    pub layout: PipelineLayout,
    pub render_pass: RenderPass,
    pub subpass: u32,
    pub base_pipeline_handle: Pipeline,
    pub base_pipeline_index: i32,
}
impl Default for GraphicsPipelineCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineShaderStageCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineShaderStageCreateFlags,
    pub stage: ShaderStageFlags,
    pub module: ShaderModule,
    pub p_name: *const c_char,
    pub p_specialization_info: *const SpecializationInfo,
}
impl Default for PipelineShaderStageCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineShaderStageCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SpecializationInfo {
    pub map_entry_count: u32,
    pub p_map_entries: *const SpecializationMapEntry,
    pub data_size: usize,
    pub p_data: *const c_void,
}
impl Default for SpecializationInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SpecializationMapEntry {
    pub constant_id: u32,
    pub offset: u32,
    pub size: usize,
}
impl Default for SpecializationMapEntry {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineVertexInputStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineVertexInputStateCreateFlags,
    pub vertex_binding_description_count: u32,
    pub p_vertex_binding_descriptions: *const VertexInputBindingDescription,
    pub vertex_attribute_description_count: u32,
    pub p_vertex_attribute_descriptions: *const VertexInputAttributeDescription,
}
impl Default for PipelineVertexInputStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineVertexInputStateCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VertexInputBindingDescription {
    pub binding: u32,
    pub stride: u32,
    pub input_rate: VertexInputRate,
}
impl Default for VertexInputBindingDescription {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type VertexInputRate = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VertexInputAttributeDescription {
    pub location: u32,
    pub binding: u32,
    pub format: Format,
    pub offset: u32,
}
impl Default for VertexInputAttributeDescription {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineInputAssemblyStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineInputAssemblyStateCreateFlags,
    pub topology: PrimitiveTopology,
    pub primitive_restart_enable: Bool32,
}
impl Default for PipelineInputAssemblyStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineInputAssemblyStateCreateFlags = u32;
pub type PrimitiveTopology = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineTessellationStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineTessellationStateCreateFlags,
    pub patch_control_points: u32,
}
impl Default for PipelineTessellationStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineTessellationStateCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineViewportStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineViewportStateCreateFlags,
    pub viewport_count: u32,
    pub p_viewports: *const Viewport,
    pub scissor_count: u32,
    pub p_scissors: *const Rect2D,
}
impl Default for PipelineViewportStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineViewportStateCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub min_depth: f32,
    pub max_depth: f32,
}
impl Default for Viewport {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Rect2D {
    pub offset: Offset2D,
    pub extent: Extent2D,
}
impl Default for Rect2D {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Offset2D {
    pub x: i32,
    pub y: i32,
}
impl Default for Offset2D {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineRasterizationStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineRasterizationStateCreateFlags,
    pub depth_clamp_enable: Bool32,
    pub rasterizer_discard_enable: Bool32,
    pub polygon_mode: PolygonMode,
    pub cull_mode: CullModeFlags,
    pub front_face: FrontFace,
    pub depth_bias_enable: Bool32,
    pub depth_bias_constant_factor: f32,
    pub depth_bias_clamp: f32,
    pub depth_bias_slope_factor: f32,
    pub line_width: f32,
}
impl Default for PipelineRasterizationStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineRasterizationStateCreateFlags = u32;
pub type PolygonMode = u32;
pub type CullModeFlags = u32;
pub type FrontFace = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineMultisampleStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineMultisampleStateCreateFlags,
    pub rasterization_samples: SampleCountFlags,
    pub sample_shading_enable: Bool32,
    pub min_sample_shading: f32,
    pub p_sample_mask: *const SampleMask,
    pub alpha_to_coverage_enable: Bool32,
    pub alpha_to_one_enable: Bool32,
}
impl Default for PipelineMultisampleStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineMultisampleStateCreateFlags = u32;
pub type SampleMask = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineDepthStencilStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineDepthStencilStateCreateFlags,
    pub depth_test_enable: Bool32,
    pub depth_write_enable: Bool32,
    pub depth_compare_op: CompareOp,
    pub depth_bounds_test_enable: Bool32,
    pub stencil_test_enable: Bool32,
    pub front: StencilOpState,
    pub back: StencilOpState,
    pub min_depth_bounds: f32,
    pub max_depth_bounds: f32,
}
impl Default for PipelineDepthStencilStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineDepthStencilStateCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StencilOpState {
    pub fail_op: StencilOp,
    pub pass_op: StencilOp,
    pub depth_fail_op: StencilOp,
    pub compare_op: CompareOp,
    pub compare_mask: u32,
    pub write_mask: u32,
    pub reference: u32,
}
impl Default for StencilOpState {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type StencilOp = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineColorBlendStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineColorBlendStateCreateFlags,
    pub logic_op_enable: Bool32,
    pub logic_op: LogicOp,
    pub attachment_count: u32,
    pub p_attachments: *const PipelineColorBlendAttachmentState,
    pub blend_constants: [f32; 4],
}
impl Default for PipelineColorBlendStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineColorBlendStateCreateFlags = u32;
pub type LogicOp = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineColorBlendAttachmentState {
    pub blend_enable: Bool32,
    pub src_color_blend_factor: BlendFactor,
    pub dst_color_blend_factor: BlendFactor,
    pub color_blend_op: BlendOp,
    pub src_alpha_blend_factor: BlendFactor,
    pub dst_alpha_blend_factor: BlendFactor,
    pub alpha_blend_op: BlendOp,
    pub color_write_mask: ColorComponentFlags,
}
impl Default for PipelineColorBlendAttachmentState {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type BlendFactor = u32;
pub type BlendOp = u32;
pub type ColorComponentFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PipelineDynamicStateCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: PipelineDynamicStateCreateFlags,
    pub dynamic_state_count: u32,
    pub p_dynamic_states: *const DynamicState,
}
impl Default for PipelineDynamicStateCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type PipelineDynamicStateCreateFlags = u32;
pub type DynamicState = u32;
pub type Pipeline = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FramebufferCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: FramebufferCreateFlags,
    pub render_pass: RenderPass,
    pub attachment_count: u32,
    pub p_attachments: *const ImageView,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
}
impl Default for FramebufferCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type FramebufferCreateFlags = u32;
pub type Framebuffer = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CommandPoolCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: CommandPoolCreateFlags,
    pub queue_family_index: u32,
}
impl Default for CommandPoolCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type CommandPoolCreateFlags = u32;
pub type CommandPool = u64;
pub type CommandPoolResetFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CommandBufferAllocateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub command_pool: CommandPool,
    pub level: CommandBufferLevel,
    pub command_buffer_count: u32,
}
impl Default for CommandBufferAllocateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type CommandBufferLevel = u32;
pub type CommandBuffer = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CommandBufferBeginInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: CommandBufferUsageFlags,
    pub p_inheritance_info: *const CommandBufferInheritanceInfo,
}
impl Default for CommandBufferBeginInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type CommandBufferUsageFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CommandBufferInheritanceInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub render_pass: RenderPass,
    pub subpass: u32,
    pub framebuffer: Framebuffer,
    pub occlusion_query_enable: Bool32,
    pub query_flags: QueryControlFlags,
    pub pipeline_statistics: QueryPipelineStatisticFlags,
}
impl Default for CommandBufferInheritanceInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type QueryControlFlags = u32;
pub type QueryPipelineStatisticFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FenceCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: FenceCreateFlags,
}
impl Default for FenceCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type FenceCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SemaphoreCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: SemaphoreCreateFlags,
}
impl Default for SemaphoreCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type SemaphoreCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SubmitInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub p_wait_semaphores: *const Semaphore,
    pub p_wait_dst_stage_mask: *const PipelineStageFlags,
    pub command_buffer_count: u32,
    pub p_command_buffers: *const CommandBuffer,
    pub signal_semaphore_count: u32,
    pub p_signal_semaphores: *const Semaphore,
}
impl Default for SubmitInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorPoolCreateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub flags: DescriptorPoolCreateFlags,
    pub max_sets: u32,
    pub pool_size_count: u32,
    pub p_pool_sizes: *const DescriptorPoolSize,
}
impl Default for DescriptorPoolCreateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DescriptorPoolCreateFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorPoolSize {
    pub ty: DescriptorType,
    pub descriptor_count: u32,
}
impl Default for DescriptorPoolSize {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DescriptorPool = u64;
pub type DescriptorPoolResetFlags = u32;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorSetAllocateInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub descriptor_pool: DescriptorPool,
    pub descriptor_set_count: u32,
    pub p_set_layouts: *const DescriptorSetLayout,
}
impl Default for DescriptorSetAllocateInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type DescriptorSet = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WriteDescriptorSet {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub dst_set: DescriptorSet,
    pub dst_binding: u32,
    pub dst_array_element: u32,
    pub descriptor_count: u32,
    pub descriptor_type: DescriptorType,
    pub p_image_info: *const DescriptorImageInfo,
    pub p_buffer_info: *const DescriptorBufferInfo,
    pub p_texel_buffer_view: *const BufferView,
}
impl Default for WriteDescriptorSet {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorImageInfo {
    pub sampler: Sampler,
    pub image_view: ImageView,
    pub image_layout: ImageLayout,
}
impl Default for DescriptorImageInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DescriptorBufferInfo {
    pub buffer: Buffer,
    pub offset: DeviceSize,
    pub range: DeviceSize,
}
impl Default for DescriptorBufferInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type BufferView = u64;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CopyDescriptorSet {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub src_set: DescriptorSet,
    pub src_binding: u32,
    pub src_array_element: u32,
    pub dst_set: DescriptorSet,
    pub dst_binding: u32,
    pub dst_array_element: u32,
    pub descriptor_count: u32,
}
impl Default for CopyDescriptorSet {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BufferCopy {
    pub src_offset: DeviceSize,
    pub dst_offset: DeviceSize,
    pub size: DeviceSize,
}
impl Default for BufferCopy {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BufferImageCopy {
    pub buffer_offset: DeviceSize,
    pub buffer_row_length: u32,
    pub buffer_image_height: u32,
    pub image_subresource: ImageSubresourceLayers,
    pub image_offset: Offset3D,
    pub image_extent: Extent3D,
}
impl Default for BufferImageCopy {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImageSubresourceLayers {
    pub aspect_mask: ImageAspectFlags,
    pub mip_level: u32,
    pub base_array_layer: u32,
    pub layer_count: u32,
}
impl Default for ImageSubresourceLayers {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Offset3D {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
impl Default for Offset3D {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MemoryBarrier {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
}
impl Default for MemoryBarrier {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BufferMemoryBarrier {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
    pub src_queue_family_index: u32,
    pub dst_queue_family_index: u32,
    pub buffer: Buffer,
    pub offset: DeviceSize,
    pub size: DeviceSize,
}
impl Default for BufferMemoryBarrier {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ImageMemoryBarrier {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
    pub old_layout: ImageLayout,
    pub new_layout: ImageLayout,
    pub src_queue_family_index: u32,
    pub dst_queue_family_index: u32,
    pub image: Image,
    pub subresource_range: ImageSubresourceRange,
}
impl Default for ImageMemoryBarrier {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RenderPassBeginInfo {
    pub s_type: StructureType,
    pub p_next: *const c_void,
    pub render_pass: RenderPass,
    pub framebuffer: Framebuffer,
    pub render_area: Rect2D,
    pub clear_value_count: u32,
    pub p_clear_values: *const ClearValue,
}
impl Default for RenderPassBeginInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union ClearValue {
    pub color: ClearColorValue,
    pub depth_stencil: ClearDepthStencilValue,
}
impl Default for ClearValue {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union ClearColorValue {
    pub float32: [f32; 4],
    pub int32: [i32; 4],
    pub uint32: [u32; 4],
}
impl Default for ClearColorValue {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ClearDepthStencilValue {
    pub depth: f32,
    pub stencil: u32,
}
impl Default for ClearDepthStencilValue {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
pub type SubpassContents = u32;
pub type CreateInstance = unsafe extern "system" fn(
    p_create_info: *const InstanceCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_instance: *mut Instance,
) -> i32;
pub type DestroyInstance =
    unsafe extern "system" fn(instance: Instance, p_allocator: *const AllocationCallbacks);
pub type EnumeratePhysicalDevices = unsafe extern "system" fn(
    instance: Instance,
    p_physical_device_count: *mut u32,
    p_physical_devices: *mut PhysicalDevice,
) -> i32;
pub type GetPhysicalDeviceQueueFamilyProperties = unsafe extern "system" fn(
    physical_device: PhysicalDevice,
    p_queue_family_property_count: *mut u32,
    p_queue_family_properties: *mut QueueFamilyProperties,
);
pub type GetPhysicalDeviceMemoryProperties = unsafe extern "system" fn(
    physical_device: PhysicalDevice,
    p_memory_properties: *mut PhysicalDeviceMemoryProperties,
);
pub type CreateDevice = unsafe extern "system" fn(
    physical_device: PhysicalDevice,
    p_create_info: *const DeviceCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_device: *mut Device,
) -> i32;
pub type DestroyDevice =
    unsafe extern "system" fn(device: Device, p_allocator: *const AllocationCallbacks);
pub type GetDeviceQueue = unsafe extern "system" fn(
    device: Device,
    queue_family_index: u32,
    queue_index: u32,
    p_queue: *mut Queue,
);
pub type DeviceWaitIdle = unsafe extern "system" fn(device: Device) -> i32;
pub type CreateXlibSurfaceKHR = unsafe extern "system" fn(
    instance: Instance,
    p_create_info: *const XlibSurfaceCreateInfoKHR,
    p_allocator: *const AllocationCallbacks,
    p_surface: *mut SurfaceKHR,
) -> i32;
pub type CreateWaylandSurfaceKHR = unsafe extern "system" fn(
    instance: Instance,
    p_create_info: *const WaylandSurfaceCreateInfoKHR,
    p_allocator: *const AllocationCallbacks,
    p_surface: *mut SurfaceKHR,
) -> i32;
pub type GetPhysicalDeviceSurfaceSupportKHR = unsafe extern "system" fn(
    physical_device: PhysicalDevice,
    queue_family_index: u32,
    surface: SurfaceKHR,
    p_supported: *mut Bool32,
) -> i32;
pub type GetPhysicalDeviceSurfaceFormatsKHR = unsafe extern "system" fn(
    physical_device: PhysicalDevice,
    surface: SurfaceKHR,
    p_surface_format_count: *mut u32,
    p_surface_formats: *mut SurfaceFormatKHR,
) -> i32;
pub type GetPhysicalDeviceSurfaceCapabilitiesKHR = unsafe extern "system" fn(
    physical_device: PhysicalDevice,
    surface: SurfaceKHR,
    p_surface_capabilities: *mut SurfaceCapabilitiesKHR,
) -> i32;
pub type DestroySurfaceKHR = unsafe extern "system" fn(
    instance: Instance,
    surface: SurfaceKHR,
    p_allocator: *const AllocationCallbacks,
);
pub type CreateSwapchainKHR = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const SwapchainCreateInfoKHR,
    p_allocator: *const AllocationCallbacks,
    p_swapchain: *mut SwapchainKHR,
) -> i32;
pub type DestroySwapchainKHR = unsafe extern "system" fn(
    device: Device,
    swapchain: SwapchainKHR,
    p_allocator: *const AllocationCallbacks,
);
pub type GetSwapchainImagesKHR = unsafe extern "system" fn(
    device: Device,
    swapchain: SwapchainKHR,
    p_swapchain_image_count: *mut u32,
    p_swapchain_images: *mut Image,
) -> i32;
pub type AcquireNextImageKHR = unsafe extern "system" fn(
    device: Device,
    swapchain: SwapchainKHR,
    timeout: u64,
    semaphore: Semaphore,
    fence: Fence,
    p_image_index: *mut u32,
) -> i32;
pub type QueuePresentKHR =
    unsafe extern "system" fn(queue: Queue, p_present_info: *const PresentInfoKHR) -> i32;
pub type CreateBuffer = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const BufferCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_buffer: *mut Buffer,
) -> i32;
pub type DestroyBuffer = unsafe extern "system" fn(
    device: Device,
    buffer: Buffer,
    p_allocator: *const AllocationCallbacks,
);
pub type GetBufferMemoryRequirements = unsafe extern "system" fn(
    device: Device,
    buffer: Buffer,
    p_memory_requirements: *mut MemoryRequirements,
);
pub type AllocateMemory = unsafe extern "system" fn(
    device: Device,
    p_allocate_info: *const MemoryAllocateInfo,
    p_allocator: *const AllocationCallbacks,
    p_memory: *mut DeviceMemory,
) -> i32;
pub type FreeMemory = unsafe extern "system" fn(
    device: Device,
    memory: DeviceMemory,
    p_allocator: *const AllocationCallbacks,
);
pub type BindBufferMemory = unsafe extern "system" fn(
    device: Device,
    buffer: Buffer,
    memory: DeviceMemory,
    memory_offset: DeviceSize,
) -> i32;
pub type MapMemory = unsafe extern "system" fn(
    device: Device,
    memory: DeviceMemory,
    offset: DeviceSize,
    size: DeviceSize,
    flags: MemoryMapFlags,
    pp_data: *mut *mut c_void,
) -> i32;
pub type UnmapMemory = unsafe extern "system" fn(device: Device, memory: DeviceMemory);
pub type CreateImage = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const ImageCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_image: *mut Image,
) -> i32;
pub type DestroyImage = unsafe extern "system" fn(
    device: Device,
    image: Image,
    p_allocator: *const AllocationCallbacks,
);
pub type GetImageMemoryRequirements = unsafe extern "system" fn(
    device: Device,
    image: Image,
    p_memory_requirements: *mut MemoryRequirements,
);
pub type BindImageMemory = unsafe extern "system" fn(
    device: Device,
    image: Image,
    memory: DeviceMemory,
    memory_offset: DeviceSize,
) -> i32;
pub type CreateImageView = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const ImageViewCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_view: *mut ImageView,
) -> i32;
pub type DestroyImageView = unsafe extern "system" fn(
    device: Device,
    image_view: ImageView,
    p_allocator: *const AllocationCallbacks,
);
pub type CreateSampler = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const SamplerCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_sampler: *mut Sampler,
) -> i32;
pub type DestroySampler = unsafe extern "system" fn(
    device: Device,
    sampler: Sampler,
    p_allocator: *const AllocationCallbacks,
);
pub type CreateShaderModule = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const ShaderModuleCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_shader_module: *mut ShaderModule,
) -> i32;
pub type DestroyShaderModule = unsafe extern "system" fn(
    device: Device,
    shader_module: ShaderModule,
    p_allocator: *const AllocationCallbacks,
);
pub type CreateDescriptorSetLayout = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const DescriptorSetLayoutCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_set_layout: *mut DescriptorSetLayout,
) -> i32;
pub type DestroyDescriptorSetLayout = unsafe extern "system" fn(
    device: Device,
    descriptor_set_layout: DescriptorSetLayout,
    p_allocator: *const AllocationCallbacks,
);
pub type CreatePipelineLayout = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const PipelineLayoutCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_pipeline_layout: *mut PipelineLayout,
) -> i32;
pub type DestroyPipelineLayout = unsafe extern "system" fn(
    device: Device,
    pipeline_layout: PipelineLayout,
    p_allocator: *const AllocationCallbacks,
);
pub type CreateRenderPass = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const RenderPassCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_render_pass: *mut RenderPass,
) -> i32;
pub type DestroyRenderPass = unsafe extern "system" fn(
    device: Device,
    render_pass: RenderPass,
    p_allocator: *const AllocationCallbacks,
);
pub type CreateGraphicsPipelines = unsafe extern "system" fn(
    device: Device,
    pipeline_cache: PipelineCache,
    create_info_count: u32,
    p_create_infos: *const GraphicsPipelineCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_pipelines: *mut Pipeline,
) -> i32;
pub type DestroyPipeline = unsafe extern "system" fn(
    device: Device,
    pipeline: Pipeline,
    p_allocator: *const AllocationCallbacks,
);
pub type CreateFramebuffer = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const FramebufferCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_framebuffer: *mut Framebuffer,
) -> i32;
pub type DestroyFramebuffer = unsafe extern "system" fn(
    device: Device,
    framebuffer: Framebuffer,
    p_allocator: *const AllocationCallbacks,
);
pub type CreateCommandPool = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const CommandPoolCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_command_pool: *mut CommandPool,
) -> i32;
pub type DestroyCommandPool = unsafe extern "system" fn(
    device: Device,
    command_pool: CommandPool,
    p_allocator: *const AllocationCallbacks,
);
pub type ResetCommandPool = unsafe extern "system" fn(
    device: Device,
    command_pool: CommandPool,
    flags: CommandPoolResetFlags,
) -> i32;
pub type AllocateCommandBuffers = unsafe extern "system" fn(
    device: Device,
    p_allocate_info: *const CommandBufferAllocateInfo,
    p_command_buffers: *mut CommandBuffer,
) -> i32;
pub type BeginCommandBuffer = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    p_begin_info: *const CommandBufferBeginInfo,
) -> i32;
pub type EndCommandBuffer = unsafe extern "system" fn(command_buffer: CommandBuffer) -> i32;
pub type CreateFence = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const FenceCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_fence: *mut Fence,
) -> i32;
pub type DestroyFence = unsafe extern "system" fn(
    device: Device,
    fence: Fence,
    p_allocator: *const AllocationCallbacks,
);
pub type WaitForFences = unsafe extern "system" fn(
    device: Device,
    fence_count: u32,
    p_fences: *const Fence,
    wait_all: Bool32,
    timeout: u64,
) -> i32;
pub type ResetFences =
    unsafe extern "system" fn(device: Device, fence_count: u32, p_fences: *const Fence) -> i32;
pub type CreateSemaphore = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const SemaphoreCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_semaphore: *mut Semaphore,
) -> i32;
pub type DestroySemaphore = unsafe extern "system" fn(
    device: Device,
    semaphore: Semaphore,
    p_allocator: *const AllocationCallbacks,
);
pub type QueueSubmit = unsafe extern "system" fn(
    queue: Queue,
    submit_count: u32,
    p_submits: *const SubmitInfo,
    fence: Fence,
) -> i32;
pub type CreateDescriptorPool = unsafe extern "system" fn(
    device: Device,
    p_create_info: *const DescriptorPoolCreateInfo,
    p_allocator: *const AllocationCallbacks,
    p_descriptor_pool: *mut DescriptorPool,
) -> i32;
pub type DestroyDescriptorPool = unsafe extern "system" fn(
    device: Device,
    descriptor_pool: DescriptorPool,
    p_allocator: *const AllocationCallbacks,
);
pub type ResetDescriptorPool = unsafe extern "system" fn(
    device: Device,
    descriptor_pool: DescriptorPool,
    flags: DescriptorPoolResetFlags,
) -> i32;
pub type AllocateDescriptorSets = unsafe extern "system" fn(
    device: Device,
    p_allocate_info: *const DescriptorSetAllocateInfo,
    p_descriptor_sets: *mut DescriptorSet,
) -> i32;
pub type UpdateDescriptorSets = unsafe extern "system" fn(
    device: Device,
    descriptor_write_count: u32,
    p_descriptor_writes: *const WriteDescriptorSet,
    descriptor_copy_count: u32,
    p_descriptor_copies: *const CopyDescriptorSet,
);
pub type CmdCopyBuffer = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    src_buffer: Buffer,
    dst_buffer: Buffer,
    region_count: u32,
    p_regions: *const BufferCopy,
);
pub type CmdCopyBufferToImage = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    src_buffer: Buffer,
    dst_image: Image,
    dst_image_layout: ImageLayout,
    region_count: u32,
    p_regions: *const BufferImageCopy,
);
pub type CmdCopyImageToBuffer = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    src_image: Image,
    src_image_layout: ImageLayout,
    dst_buffer: Buffer,
    region_count: u32,
    p_regions: *const BufferImageCopy,
);
pub type CmdPipelineBarrier = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    src_stage_mask: PipelineStageFlags,
    dst_stage_mask: PipelineStageFlags,
    dependency_flags: DependencyFlags,
    memory_barrier_count: u32,
    p_memory_barriers: *const MemoryBarrier,
    buffer_memory_barrier_count: u32,
    p_buffer_memory_barriers: *const BufferMemoryBarrier,
    image_memory_barrier_count: u32,
    p_image_memory_barriers: *const ImageMemoryBarrier,
);
pub type CmdBeginRenderPass = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    p_render_pass_begin: *const RenderPassBeginInfo,
    contents: SubpassContents,
);
pub type CmdEndRenderPass = unsafe extern "system" fn(command_buffer: CommandBuffer);
pub type CmdBindPipeline = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    pipeline_bind_point: PipelineBindPoint,
    pipeline: Pipeline,
);
pub type CmdBindDescriptorSets = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    pipeline_bind_point: PipelineBindPoint,
    layout: PipelineLayout,
    first_set: u32,
    descriptor_set_count: u32,
    p_descriptor_sets: *const DescriptorSet,
    dynamic_offset_count: u32,
    p_dynamic_offsets: *const u32,
);
pub type CmdBindVertexBuffers = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    first_binding: u32,
    binding_count: u32,
    p_buffers: *const Buffer,
    p_offsets: *const DeviceSize,
);
pub type CmdSetViewport = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    first_viewport: u32,
    viewport_count: u32,
    p_viewports: *const Viewport,
);
pub type CmdSetScissor = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    first_scissor: u32,
    scissor_count: u32,
    p_scissors: *const Rect2D,
);
pub type CmdDraw = unsafe extern "system" fn(
    command_buffer: CommandBuffer,
    vertex_count: u32,
    instance_count: u32,
    first_vertex: u32,
    first_instance: u32,
);

// The BSDs provide the dynamic loader functions in libc.
#[cfg_attr(target_os = "linux", link(name = "dl"))]
unsafe extern "C" {
    pub(super) fn dlopen(path: *const c_char, flags: i32) -> *mut c_void;
    pub(super) fn dlsym(library: *mut c_void, name: *const c_char) -> *mut c_void;
    pub(super) fn dlclose(library: *mut c_void) -> i32;
}

#[cfg(feature = "window")]
unsafe extern "C" {
    pub(super) fn gtk_widget_get_window(widget: *mut c_void) -> *mut c_void;
    pub(super) fn gtk_widget_get_scale_factor(widget: *mut c_void) -> i32;
    pub(super) fn gtk_widget_get_frame_clock(widget: *mut c_void) -> *mut c_void;
    pub(super) fn gtk_widget_add_tick_callback(
        widget: *mut c_void,
        callback: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
        data: *mut c_void,
        notify: extern "C" fn(*mut c_void),
    ) -> u32;
    pub(super) fn gtk_widget_remove_tick_callback(widget: *mut c_void, id: u32);
    pub(super) fn gdk_window_get_display(window: *mut c_void) -> *mut c_void;
    pub(super) fn gdk_x11_display_get_xdisplay(display: *mut c_void) -> *mut c_void;
    pub(super) fn gdk_x11_window_get_xid(window: *mut c_void) -> c_ulong;
    pub(super) fn gdk_wayland_display_get_wl_display(display: *mut c_void) -> *mut c_void;
    pub(super) fn gdk_wayland_window_get_wl_surface(window: *mut c_void) -> *mut c_void;
}

pub(super) type GetProc = unsafe extern "system" fn(Instance, *const c_char) -> *const c_void;

#[repr(C)]
#[derive(Default)]
pub(super) struct Allocation {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct WindowAttributes {
    pub title: *mut c_char,
    pub event_mask: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub wclass: i32,
    pub visual: *mut c_void,
    pub window_type: i32,
    pub cursor: *mut c_void,
    pub wmclass_name: *mut c_char,
    pub wmclass_class: *mut c_char,
    pub override_redirect: i32,
    pub type_hint: i32,
}

#[cfg(feature = "window")]
unsafe extern "C" {
    pub(super) fn gtk_drawing_area_new() -> *mut c_void;
    pub(super) fn g_object_ref_sink(object: *mut c_void) -> *mut c_void;
    pub(super) fn gtk_widget_set_can_focus(widget: *mut c_void, focus: i32);
    pub(super) fn gtk_widget_add_events(widget: *mut c_void, events: i32);
    pub(super) fn gtk_widget_grab_focus(widget: *mut c_void);
    pub(super) fn gtk_widget_realize(widget: *mut c_void);
    pub(super) fn gtk_container_check_resize(widget: *mut c_void);
    pub(super) fn gtk_widget_get_allocated_width(widget: *mut c_void) -> i32;
    pub(super) fn gtk_widget_get_allocated_height(widget: *mut c_void) -> i32;
    pub(super) fn gtk_widget_get_allocation(widget: *mut c_void, allocation: *mut Allocation);
    pub(super) fn gtk_widget_get_parent_window(widget: *mut c_void) -> *mut c_void;
    pub(super) fn gtk_widget_get_visual(widget: *mut c_void) -> *mut c_void;
    pub(super) fn gtk_widget_get_events(widget: *mut c_void) -> i32;
    pub(super) fn gtk_widget_set_window(widget: *mut c_void, window: *mut c_void);
    pub(super) fn gtk_widget_register_window(widget: *mut c_void, window: *mut c_void);
    pub(super) fn gtk_widget_unregister_window(widget: *mut c_void, window: *mut c_void);
    pub(super) fn gdk_window_new(
        parent: *mut c_void,
        attributes: *const WindowAttributes,
        mask: i32,
    ) -> *mut c_void;
    pub(super) fn gdk_window_set_transient_for(window: *mut c_void, parent: *mut c_void);
    pub(super) fn gdk_window_destroy(window: *mut c_void);
    pub(super) fn gdk_window_ensure_native(window: *mut c_void) -> i32;
    pub(super) fn gdk_wayland_display_get_type() -> usize;
    pub(super) fn g_type_check_instance_is_a(instance: *mut c_void, type_: usize) -> i32;
}

#[cfg(feature = "window")]
unsafe extern "C" {
    pub(super) fn g_object_get_data(object: *mut c_void, key: *const c_char) -> *mut c_void;
    pub(super) fn g_object_set_data(object: *mut c_void, key: *const c_char, data: *mut c_void);
    pub(super) fn gdk_window_set_invalidate_handler(
        window: *mut c_void,
        handler: Option<extern "C" fn(*mut c_void, *mut c_void)>,
    );
    pub(super) fn gdk_window_get_update_area(window: *mut c_void) -> *mut c_void;
}

#[cfg(feature = "window")]
// The BSDs only ship versioned library names, which the linker resolves from `cairo`.
#[cfg_attr(
    target_os = "linux",
    link(name = "libcairo.so.2", kind = "dylib", modifiers = "+verbatim")
)]
#[cfg_attr(not(target_os = "linux"), link(name = "cairo"))]
unsafe extern "C" {
    pub(super) fn cairo_region_subtract(region: *mut c_void, other: *mut c_void) -> i32;
    pub(super) fn cairo_region_destroy(region: *mut c_void);
    #[cfg(test)]
    pub(super) fn cairo_region_is_empty(region: *mut c_void) -> i32;
}
