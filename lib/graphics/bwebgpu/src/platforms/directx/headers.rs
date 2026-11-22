/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Minimal Direct3D 12 and DXGI ABI. Only declarations used by bwebgpu are included.
#![allow(missing_docs, reason = "Raw Direct3D 12 and DXGI declarations")]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::upper_case_acronyms)]
#![allow(unused)]

use std::ffi::{c_char, c_void};

pub(super) type BOOL = i32;
pub(super) type HANDLE = *mut c_void;
pub(super) type HWND = HANDLE;
pub(super) type HRESULT = i32;

#[repr(C)]
#[derive(PartialEq, Eq)]
pub(super) struct GUID {
    pub(super) data1: u32,
    pub(super) data2: u16,
    pub(super) data3: u16,
    pub(super) data4: [u8; 8],
}

#[repr(C)]
#[derive(Default, Clone)]
pub(super) struct RECT {
    pub(super) left: i32,
    pub(super) top: i32,
    pub(super) right: i32,
    pub(super) bottom: i32,
}

#[repr(C)]
pub(super) struct IUnknownVtbl {
    pub(super) QueryInterface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    pub(super) AddRef: unsafe extern "system" fn(*mut c_void) -> u32,
    pub(super) Release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct ID3D12ObjectVtbl {
    base: IUnknownVtbl,
    _unused_3_6: [*const c_void; 4],
}

#[repr(C)]
struct ID3D12DeviceChildVtbl {
    base: ID3D12ObjectVtbl,
    _unused_7_7: [*const c_void; 1],
}

#[repr(C)]
struct ID3D12PageableVtbl {
    base: ID3D12DeviceChildVtbl,
}

#[repr(C)]
struct ID3D12CommandListVtbl {
    base: ID3D12DeviceChildVtbl,
    _unused_8_8: [*const c_void; 1],
}

#[repr(C)]
struct IDXGIObjectVtbl {
    base: IUnknownVtbl,
    _unused_3_6: [*const c_void; 4],
}

#[repr(C)]
struct IDXGIDeviceSubObjectVtbl {
    base: IDXGIObjectVtbl,
    _unused_7_7: [*const c_void; 1],
}

#[repr(C)]
struct IDXGIFactory {
    vtbl: *const IDXGIFactoryVtbl,
}

#[repr(C)]
struct IDXGIFactoryVtbl {
    base: IDXGIObjectVtbl,
    _unused_7_7: [*const c_void; 1],
    MakeWindowAssociation:
        unsafe extern "system" fn(*mut IDXGIFactory, HWND, DXGI_MWA_FLAGS) -> i32,
    _unused_9_11: [*const c_void; 3],
}

#[repr(C)]
struct IDXGIFactory1 {
    vtbl: *const IDXGIFactory1Vtbl,
}

#[repr(C)]
struct IDXGIFactory1Vtbl {
    base: IDXGIFactoryVtbl,
    EnumAdapters1: unsafe extern "system" fn(*mut IDXGIFactory1, u32, *mut *mut c_void) -> HRESULT,
    _unused_13_13: [*const c_void; 1],
}

#[repr(C)]
struct IDXGIFactory2 {
    vtbl: *const IDXGIFactory2Vtbl,
}

#[repr(C)]
struct IDXGIFactory2Vtbl {
    base: IDXGIFactory1Vtbl,
    _unused_14_14: [*const c_void; 1],
    CreateSwapChainForHwnd: unsafe extern "system" fn(
        *mut IDXGIFactory2,
        *mut c_void,
        HWND,
        *const DXGI_SWAP_CHAIN_DESC1,
        *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
        *mut c_void,
        *mut *mut c_void,
    ) -> i32,
    _unused_16_23: [*const c_void; 8],
    CreateSwapChainForComposition: unsafe extern "system" fn(
        *mut IDXGIFactory2,
        *mut c_void,
        *const DXGI_SWAP_CHAIN_DESC1,
        *mut c_void,
        *mut *mut c_void,
    ) -> i32,
}

#[repr(C)]
struct IDXGIFactory3 {
    vtbl: *const IDXGIFactory3Vtbl,
}

#[repr(C)]
struct IDXGIFactory3Vtbl {
    base: IDXGIFactory2Vtbl,
    _unused_25_25: [*const c_void; 1],
}

#[repr(C)]
pub(super) struct ID3D12Device {
    vtbl: *const ID3D12DeviceVtbl,
}

#[repr(C)]
struct ID3D12DeviceVtbl {
    base: ID3D12ObjectVtbl,
    _unused_7_7: [*const c_void; 1],
    CreateCommandQueue: unsafe extern "system" fn(
        *mut ID3D12Device,
        *const D3D12_COMMAND_QUEUE_DESC,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    CreateCommandAllocator: unsafe extern "system" fn(
        *mut ID3D12Device,
        D3D12_COMMAND_LIST_TYPE,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    CreateGraphicsPipelineState: unsafe extern "system" fn(
        *mut ID3D12Device,
        *const D3D12_GRAPHICS_PIPELINE_STATE_DESC,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    _unused_11_11: [*const c_void; 1],
    CreateCommandList: unsafe extern "system" fn(
        *mut ID3D12Device,
        u32,
        D3D12_COMMAND_LIST_TYPE,
        *mut c_void,
        *mut c_void,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    _unused_13_13: [*const c_void; 1],
    CreateDescriptorHeap: unsafe extern "system" fn(
        *mut ID3D12Device,
        *const D3D12_DESCRIPTOR_HEAP_DESC,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    GetDescriptorHandleIncrementSize:
        unsafe extern "system" fn(*mut ID3D12Device, D3D12_DESCRIPTOR_HEAP_TYPE) -> u32,
    CreateRootSignature: unsafe extern "system" fn(
        *mut ID3D12Device,
        u32,
        *const c_void,
        usize,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    _unused_17_17: [*const c_void; 1],
    CreateShaderResourceView: unsafe extern "system" fn(
        *mut ID3D12Device,
        *mut c_void,
        *const D3D12_SHADER_RESOURCE_VIEW_DESC,
        D3D12_CPU_DESCRIPTOR_HANDLE,
    ),
    _unused_19_19: [*const c_void; 1],
    CreateRenderTargetView: unsafe extern "system" fn(
        *mut ID3D12Device,
        *mut c_void,
        *const D3D12_RENDER_TARGET_VIEW_DESC,
        D3D12_CPU_DESCRIPTOR_HANDLE,
    ),
    CreateDepthStencilView: unsafe extern "system" fn(
        *mut ID3D12Device,
        *mut c_void,
        *const D3D12_DEPTH_STENCIL_VIEW_DESC,
        D3D12_CPU_DESCRIPTOR_HANDLE,
    ),
    CreateSampler: unsafe extern "system" fn(
        *mut ID3D12Device,
        *const D3D12_SAMPLER_DESC,
        D3D12_CPU_DESCRIPTOR_HANDLE,
    ),
    _unused_23_26: [*const c_void; 4],
    CreateCommittedResource: unsafe extern "system" fn(
        *mut ID3D12Device,
        *const D3D12_HEAP_PROPERTIES,
        D3D12_HEAP_FLAGS,
        *const D3D12_RESOURCE_DESC,
        D3D12_RESOURCE_STATES,
        *const D3D12_CLEAR_VALUE,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    _unused_28_35: [*const c_void; 8],
    CreateFence: unsafe extern "system" fn(
        *mut ID3D12Device,
        u64,
        D3D12_FENCE_FLAGS,
        *const GUID,
        *mut *mut c_void,
    ) -> i32,
    GetDeviceRemovedReason: unsafe extern "system" fn(*mut ID3D12Device) -> i32,
}

#[repr(C)]
pub(super) struct ID3D12CommandQueue {
    vtbl: *const ID3D12CommandQueueVtbl,
}

#[repr(C)]
struct ID3D12CommandQueueVtbl {
    base: ID3D12PageableVtbl,
    _unused_8_9: [*const c_void; 2],
    ExecuteCommandLists:
        unsafe extern "system" fn(*mut ID3D12CommandQueue, u32, *const *mut c_void),
    _unused_11_13: [*const c_void; 3],
    Signal: unsafe extern "system" fn(*mut ID3D12CommandQueue, *mut c_void, u64) -> i32,
}

#[repr(C)]
pub(super) struct ID3D12CommandAllocator {
    vtbl: *const ID3D12CommandAllocatorVtbl,
}

#[repr(C)]
struct ID3D12CommandAllocatorVtbl {
    base: ID3D12PageableVtbl,
    Reset: unsafe extern "system" fn(*mut ID3D12CommandAllocator) -> i32,
}

#[repr(C)]
pub(super) struct ID3D12GraphicsCommandList {
    vtbl: *const ID3D12GraphicsCommandListVtbl,
}

#[repr(C)]
struct ID3D12GraphicsCommandListVtbl {
    base: ID3D12CommandListVtbl,
    Close: unsafe extern "system" fn(*mut ID3D12GraphicsCommandList) -> i32,
    Reset:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, *mut c_void, *mut c_void) -> i32,
    _unused_11_11: [*const c_void; 1],
    DrawInstanced: unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, u32, u32, u32),
    DrawIndexedInstanced:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, u32, u32, i32, u32),
    _unused_14_14: [*const c_void; 1],
    CopyBufferRegion: unsafe extern "system" fn(
        *mut ID3D12GraphicsCommandList,
        *mut c_void,
        u64,
        *mut c_void,
        u64,
        u64,
    ),
    CopyTextureRegion: unsafe extern "system" fn(
        *mut ID3D12GraphicsCommandList,
        *const D3D12_TEXTURE_COPY_LOCATION,
        u32,
        u32,
        u32,
        *const D3D12_TEXTURE_COPY_LOCATION,
        *const D3D12_BOX,
    ),
    _unused_17_19: [*const c_void; 3],
    IASetPrimitiveTopology:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, D3D_PRIMITIVE_TOPOLOGY),
    RSSetViewports:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, *const D3D12_VIEWPORT),
    RSSetScissorRects: unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, *const RECT),
    _unused_23_24: [*const c_void; 2],
    SetPipelineState: unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, *mut c_void),
    ResourceBarrier: unsafe extern "system" fn(
        *mut ID3D12GraphicsCommandList,
        u32,
        *const D3D12_RESOURCE_BARRIER,
    ),
    _unused_27_27: [*const c_void; 1],
    SetDescriptorHeaps:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, *const *mut c_void),
    _unused_29_29: [*const c_void; 1],
    SetGraphicsRootSignature:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, *mut c_void),
    _unused_31_31: [*const c_void; 1],
    SetGraphicsRootDescriptorTable:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, D3D12_GPU_DESCRIPTOR_HANDLE),
    _unused_33_35: [*const c_void; 3],
    SetGraphicsRoot32BitConstants:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, u32, *const c_void, u32),
    _unused_37_37: [*const c_void; 1],
    SetGraphicsRootConstantBufferView:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, u64),
    _unused_39_39: [*const c_void; 1],
    SetGraphicsRootShaderResourceView:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, u32, u64),
    _unused_41_42: [*const c_void; 2],
    IASetIndexBuffer:
        unsafe extern "system" fn(*mut ID3D12GraphicsCommandList, *const D3D12_INDEX_BUFFER_VIEW),
    IASetVertexBuffers: unsafe extern "system" fn(
        *mut ID3D12GraphicsCommandList,
        u32,
        u32,
        *const D3D12_VERTEX_BUFFER_VIEW,
    ),
    _unused_45_45: [*const c_void; 1],
    OMSetRenderTargets: unsafe extern "system" fn(
        *mut ID3D12GraphicsCommandList,
        u32,
        *const D3D12_CPU_DESCRIPTOR_HANDLE,
        i32,
        *const D3D12_CPU_DESCRIPTOR_HANDLE,
    ),
    ClearDepthStencilView: unsafe extern "system" fn(
        *mut ID3D12GraphicsCommandList,
        D3D12_CPU_DESCRIPTOR_HANDLE,
        D3D12_CLEAR_FLAGS,
        f32,
        u8,
        u32,
        *const RECT,
    ),
    ClearRenderTargetView: unsafe extern "system" fn(
        *mut ID3D12GraphicsCommandList,
        D3D12_CPU_DESCRIPTOR_HANDLE,
        *const f32,
        u32,
        *const RECT,
    ),
}

#[repr(C)]
pub(super) struct ID3D12Resource {
    vtbl: *const ID3D12ResourceVtbl,
}

#[repr(C)]
struct ID3D12ResourceVtbl {
    base: ID3D12PageableVtbl,
    Map: unsafe extern "system" fn(
        *mut ID3D12Resource,
        u32,
        *const D3D12_RANGE,
        *mut *mut c_void,
    ) -> i32,
    Unmap: unsafe extern "system" fn(*mut ID3D12Resource, u32, *const D3D12_RANGE),
    _unused_10_10: [*const c_void; 1],
    GetGPUVirtualAddress: unsafe extern "system" fn(*mut ID3D12Resource) -> u64,
}

#[repr(C)]
pub(super) struct ID3D12DescriptorHeap {
    vtbl: *const ID3D12DescriptorHeapVtbl,
}

#[repr(C)]
struct ID3D12DescriptorHeapVtbl {
    base: ID3D12PageableVtbl,
    _unused_8_8: [*const c_void; 1],
    GetCPUDescriptorHandleForHeapStart:
        unsafe extern "system" fn(*mut ID3D12DescriptorHeap, *mut D3D12_CPU_DESCRIPTOR_HANDLE),
    GetGPUDescriptorHandleForHeapStart:
        unsafe extern "system" fn(*mut ID3D12DescriptorHeap, *mut D3D12_GPU_DESCRIPTOR_HANDLE),
}

#[repr(C)]
pub(super) struct ID3D12Fence {
    vtbl: *const ID3D12FenceVtbl,
}

#[repr(C)]
struct ID3D12FenceVtbl {
    base: ID3D12PageableVtbl,
    GetCompletedValue: unsafe extern "system" fn(*mut ID3D12Fence) -> u64,
    SetEventOnCompletion: unsafe extern "system" fn(*mut ID3D12Fence, u64, HANDLE) -> i32,
}

#[repr(C)]
pub(super) struct IDXGIFactory4 {
    vtbl: *const IDXGIFactory4Vtbl,
}

#[repr(C)]
struct IDXGIFactory4Vtbl {
    base: IDXGIFactory3Vtbl,
    _unused_26_26: [*const c_void; 1],
    EnumWarpAdapter:
        unsafe extern "system" fn(*mut IDXGIFactory4, *const GUID, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IDCompositionVisual {
    vtbl: *const IDCompositionVisualVtbl,
}

#[repr(C)]
struct IDCompositionVisualVtbl {
    base: IUnknownVtbl,
    _unused_3_14: [*const c_void; 12],
    SetContent: unsafe extern "system" fn(*mut IDCompositionVisual, *mut c_void) -> i32,
}

#[repr(C)]
struct IDCompositionDevice {
    vtbl: *const IDCompositionDeviceVtbl,
}

#[repr(C)]
struct IDCompositionDeviceVtbl {
    base: IUnknownVtbl,
    Commit: unsafe extern "system" fn(*mut IDCompositionDevice) -> i32,
}

#[repr(C)]
struct IDXGISwapChain {
    vtbl: *const IDXGISwapChainVtbl,
}

#[repr(C)]
struct IDXGISwapChainVtbl {
    base: IDXGIDeviceSubObjectVtbl,
    Present: unsafe extern "system" fn(*mut IDXGISwapChain, u32, DXGI_PRESENT) -> i32,
    GetBuffer:
        unsafe extern "system" fn(*mut IDXGISwapChain, u32, *const GUID, *mut *mut c_void) -> i32,
    _unused_10_12: [*const c_void; 3],
    ResizeBuffers:
        unsafe extern "system" fn(*mut IDXGISwapChain, u32, u32, u32, DXGI_FORMAT, u32) -> i32,
    _unused_14_17: [*const c_void; 4],
}

#[repr(C)]
pub(super) struct IDXGISwapChain1 {
    vtbl: *const IDXGISwapChain1Vtbl,
}

#[repr(C)]
struct IDXGISwapChain1Vtbl {
    base: IDXGISwapChainVtbl,
    _unused_18_28: [*const c_void; 11],
}

#[repr(C)]
struct IDXGISwapChain2Vtbl {
    base: IDXGISwapChain1Vtbl,
    _unused_29_35: [*const c_void; 7],
}

#[repr(C)]
pub(super) struct IDXGISwapChain3 {
    vtbl: *const IDXGISwapChain3Vtbl,
}

#[repr(C)]
struct IDXGISwapChain3Vtbl {
    base: IDXGISwapChain2Vtbl,
    GetCurrentBackBufferIndex: unsafe extern "system" fn(*mut IDXGISwapChain3) -> u32,
}

#[repr(C)]
pub(super) struct ID3DBlob {
    vtbl: *const ID3DBlobVtbl,
}

#[repr(C)]
struct ID3DBlobVtbl {
    base: IUnknownVtbl,
    GetBufferPointer: unsafe extern "system" fn(*mut ID3DBlob) -> *mut c_void,
    GetBufferSize: unsafe extern "system" fn(*mut ID3DBlob) -> usize,
}

#[repr(C)]
pub(super) struct IDXGIAdapter1 {
    vtbl: *const IDXGIAdapter1Vtbl,
}

#[repr(C)]
struct IDXGIAdapterVtbl {
    base: IDXGIObjectVtbl,
    _unused_7_9: [*const c_void; 3],
}

#[repr(C)]
struct IDXGIAdapter1Vtbl {
    base: IDXGIAdapterVtbl,
    GetDesc1: unsafe extern "system" fn(*mut IDXGIAdapter1, *mut DXGI_ADAPTER_DESC1) -> HRESULT,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_COMMAND_QUEUE_DESC {
    pub(super) Type: D3D12_COMMAND_LIST_TYPE,
    pub(super) Priority: i32,
    pub(super) Flags: D3D12_COMMAND_QUEUE_FLAGS,
    pub(super) NodeMask: u32,
}
pub(super) type D3D12_COMMAND_LIST_TYPE = i32;
pub(super) type D3D12_COMMAND_QUEUE_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_GRAPHICS_PIPELINE_STATE_DESC {
    pub(super) pRootSignature: *mut c_void,
    pub(super) VS: D3D12_SHADER_BYTECODE,
    pub(super) PS: D3D12_SHADER_BYTECODE,
    pub(super) DS: D3D12_SHADER_BYTECODE,
    pub(super) HS: D3D12_SHADER_BYTECODE,
    pub(super) GS: D3D12_SHADER_BYTECODE,
    pub(super) StreamOutput: D3D12_STREAM_OUTPUT_DESC,
    pub(super) BlendState: D3D12_BLEND_DESC,
    pub(super) SampleMask: u32,
    pub(super) RasterizerState: D3D12_RASTERIZER_DESC,
    pub(super) DepthStencilState: D3D12_DEPTH_STENCIL_DESC,
    pub(super) InputLayout: D3D12_INPUT_LAYOUT_DESC,
    pub(super) IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE,
    pub(super) PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE,
    pub(super) NumRenderTargets: u32,
    pub(super) RTVFormats: [DXGI_FORMAT; 8],
    pub(super) DSVFormat: DXGI_FORMAT,
    pub(super) SampleDesc: DXGI_SAMPLE_DESC,
    pub(super) NodeMask: u32,
    pub(super) CachedPSO: D3D12_CACHED_PIPELINE_STATE,
    pub(super) Flags: D3D12_PIPELINE_STATE_FLAGS,
}
#[repr(C)]
pub(super) struct ID3D12RootSignature {
    vtbl: *const ID3D12DeviceChildVtbl,
}

#[repr(C)]
pub(super) struct ID3D12PipelineState {
    vtbl: *const ID3D12PageableVtbl,
}

#[repr(C)]
pub(super) struct IDXGIAdapter {
    vtbl: *const IDXGIAdapterVtbl,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_SHADER_BYTECODE {
    pub(super) pShaderBytecode: *const c_void,
    pub(super) BytecodeLength: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_STREAM_OUTPUT_DESC {
    pub(super) pSODeclaration: *const D3D12_SO_DECLARATION_ENTRY,
    pub(super) NumEntries: u32,
    pub(super) pBufferStrides: *const u32,
    pub(super) NumStrides: u32,
    pub(super) RasterizedStream: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_SO_DECLARATION_ENTRY {
    pub(super) Stream: u32,
    pub(super) SemanticName: PCSTR,
    pub(super) SemanticIndex: u32,
    pub(super) StartComponent: u8,
    pub(super) ComponentCount: u8,
    pub(super) OutputSlot: u8,
}
pub(super) type PCSTR = *const u8;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_BLEND_DESC {
    pub(super) AlphaToCoverageEnable: i32,
    pub(super) IndependentBlendEnable: i32,
    pub(super) RenderTarget: [D3D12_RENDER_TARGET_BLEND_DESC; 8],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RENDER_TARGET_BLEND_DESC {
    pub(super) BlendEnable: i32,
    pub(super) LogicOpEnable: i32,
    pub(super) SrcBlend: D3D12_BLEND,
    pub(super) DestBlend: D3D12_BLEND,
    pub(super) BlendOp: D3D12_BLEND_OP,
    pub(super) SrcBlendAlpha: D3D12_BLEND,
    pub(super) DestBlendAlpha: D3D12_BLEND,
    pub(super) BlendOpAlpha: D3D12_BLEND_OP,
    pub(super) LogicOp: D3D12_LOGIC_OP,
    pub(super) RenderTargetWriteMask: u8,
}
pub(super) type D3D12_BLEND = i32;
pub(super) type D3D12_BLEND_OP = i32;
pub(super) type D3D12_LOGIC_OP = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RASTERIZER_DESC {
    pub(super) FillMode: D3D12_FILL_MODE,
    pub(super) CullMode: D3D12_CULL_MODE,
    pub(super) FrontCounterClockwise: i32,
    pub(super) DepthBias: i32,
    pub(super) DepthBiasClamp: f32,
    pub(super) SlopeScaledDepthBias: f32,
    pub(super) DepthClipEnable: i32,
    pub(super) MultisampleEnable: i32,
    pub(super) AntialiasedLineEnable: i32,
    pub(super) ForcedSampleCount: u32,
    pub(super) ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE,
}
pub(super) type D3D12_FILL_MODE = i32;
pub(super) type D3D12_CULL_MODE = i32;
pub(super) type D3D12_CONSERVATIVE_RASTERIZATION_MODE = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_DEPTH_STENCIL_DESC {
    pub(super) DepthEnable: i32,
    pub(super) DepthWriteMask: D3D12_DEPTH_WRITE_MASK,
    pub(super) DepthFunc: D3D12_COMPARISON_FUNC,
    pub(super) StencilEnable: i32,
    pub(super) StencilReadMask: u8,
    pub(super) StencilWriteMask: u8,
    pub(super) FrontFace: D3D12_DEPTH_STENCILOP_DESC,
    pub(super) BackFace: D3D12_DEPTH_STENCILOP_DESC,
}
pub(super) type D3D12_DEPTH_WRITE_MASK = i32;
pub(super) type D3D12_COMPARISON_FUNC = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_DEPTH_STENCILOP_DESC {
    pub(super) StencilFailOp: D3D12_STENCIL_OP,
    pub(super) StencilDepthFailOp: D3D12_STENCIL_OP,
    pub(super) StencilPassOp: D3D12_STENCIL_OP,
    pub(super) StencilFunc: D3D12_COMPARISON_FUNC,
}
pub(super) type D3D12_STENCIL_OP = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_INPUT_LAYOUT_DESC {
    pub(super) pInputElementDescs: *const D3D12_INPUT_ELEMENT_DESC,
    pub(super) NumElements: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_INPUT_ELEMENT_DESC {
    pub(super) SemanticName: PCSTR,
    pub(super) SemanticIndex: u32,
    pub(super) Format: DXGI_FORMAT,
    pub(super) InputSlot: u32,
    pub(super) AlignedByteOffset: u32,
    pub(super) InputSlotClass: D3D12_INPUT_CLASSIFICATION,
    pub(super) InstanceDataStepRate: u32,
}
pub(super) type DXGI_FORMAT = i32;
pub(super) type D3D12_INPUT_CLASSIFICATION = i32;
pub(super) type D3D12_INDEX_BUFFER_STRIP_CUT_VALUE = i32;
pub(super) type D3D12_PRIMITIVE_TOPOLOGY_TYPE = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DXGI_SAMPLE_DESC {
    pub(super) Count: u32,
    pub(super) Quality: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_CACHED_PIPELINE_STATE {
    pub(super) pCachedBlob: *const c_void,
    pub(super) CachedBlobSizeInBytes: usize,
}
pub(super) type D3D12_PIPELINE_STATE_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_DESCRIPTOR_HEAP_DESC {
    pub(super) Type: D3D12_DESCRIPTOR_HEAP_TYPE,
    pub(super) NumDescriptors: u32,
    pub(super) Flags: D3D12_DESCRIPTOR_HEAP_FLAGS,
    pub(super) NodeMask: u32,
}
pub(super) type D3D12_DESCRIPTOR_HEAP_TYPE = i32;
pub(super) type D3D12_DESCRIPTOR_HEAP_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_SHADER_RESOURCE_VIEW_DESC {
    pub(super) Format: DXGI_FORMAT,
    pub(super) ViewDimension: D3D12_SRV_DIMENSION,
    pub(super) Shader4ComponentMapping: u32,
    pub(super) Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0,
}
pub(super) type D3D12_SRV_DIMENSION = i32;
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
    pub(super) Buffer: D3D12_BUFFER_SRV,
    pub(super) Texture1D: D3D12_TEX1D_SRV,
    pub(super) Texture1DArray: D3D12_TEX1D_ARRAY_SRV,
    pub(super) Texture2D: D3D12_TEX2D_SRV,
    pub(super) Texture2DArray: D3D12_TEX2D_ARRAY_SRV,
    pub(super) Texture2DMS: D3D12_TEX2DMS_SRV,
    pub(super) Texture2DMSArray: D3D12_TEX2DMS_ARRAY_SRV,
    pub(super) Texture3D: D3D12_TEX3D_SRV,
    pub(super) TextureCube: D3D12_TEXCUBE_SRV,
    pub(super) TextureCubeArray: D3D12_TEXCUBE_ARRAY_SRV,
    pub(super) RaytracingAccelerationStructure: D3D12_RAYTRACING_ACCELERATION_STRUCTURE_SRV,
}
impl Default for D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_BUFFER_SRV {
    pub(super) FirstElement: u64,
    pub(super) NumElements: u32,
    pub(super) StructureByteStride: u32,
    pub(super) Flags: D3D12_BUFFER_SRV_FLAGS,
}
pub(super) type D3D12_BUFFER_SRV_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX1D_SRV {
    pub(super) MostDetailedMip: u32,
    pub(super) MipLevels: u32,
    pub(super) ResourceMinLODClamp: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX1D_ARRAY_SRV {
    pub(super) MostDetailedMip: u32,
    pub(super) MipLevels: u32,
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
    pub(super) ResourceMinLODClamp: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2D_SRV {
    pub(super) MostDetailedMip: u32,
    pub(super) MipLevels: u32,
    pub(super) PlaneSlice: u32,
    pub(super) ResourceMinLODClamp: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2D_ARRAY_SRV {
    pub(super) MostDetailedMip: u32,
    pub(super) MipLevels: u32,
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
    pub(super) PlaneSlice: u32,
    pub(super) ResourceMinLODClamp: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2DMS_SRV {
    pub(super) UnusedField_NothingToDefine: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2DMS_ARRAY_SRV {
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX3D_SRV {
    pub(super) MostDetailedMip: u32,
    pub(super) MipLevels: u32,
    pub(super) ResourceMinLODClamp: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEXCUBE_SRV {
    pub(super) MostDetailedMip: u32,
    pub(super) MipLevels: u32,
    pub(super) ResourceMinLODClamp: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEXCUBE_ARRAY_SRV {
    pub(super) MostDetailedMip: u32,
    pub(super) MipLevels: u32,
    pub(super) First2DArrayFace: u32,
    pub(super) NumCubes: u32,
    pub(super) ResourceMinLODClamp: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RAYTRACING_ACCELERATION_STRUCTURE_SRV {
    pub(super) Location: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_CPU_DESCRIPTOR_HANDLE {
    pub(super) ptr: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RENDER_TARGET_VIEW_DESC {
    pub(super) Format: DXGI_FORMAT,
    pub(super) ViewDimension: D3D12_RTV_DIMENSION,
    pub(super) Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0,
}
pub(super) type D3D12_RTV_DIMENSION = i32;
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union D3D12_RENDER_TARGET_VIEW_DESC_0 {
    pub(super) Buffer: D3D12_BUFFER_RTV,
    pub(super) Texture1D: D3D12_TEX1D_RTV,
    pub(super) Texture1DArray: D3D12_TEX1D_ARRAY_RTV,
    pub(super) Texture2D: D3D12_TEX2D_RTV,
    pub(super) Texture2DArray: D3D12_TEX2D_ARRAY_RTV,
    pub(super) Texture2DMS: D3D12_TEX2DMS_RTV,
    pub(super) Texture2DMSArray: D3D12_TEX2DMS_ARRAY_RTV,
    pub(super) Texture3D: D3D12_TEX3D_RTV,
}
impl Default for D3D12_RENDER_TARGET_VIEW_DESC_0 {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_BUFFER_RTV {
    pub(super) FirstElement: u64,
    pub(super) NumElements: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX1D_RTV {
    pub(super) MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX1D_ARRAY_RTV {
    pub(super) MipSlice: u32,
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2D_RTV {
    pub(super) MipSlice: u32,
    pub(super) PlaneSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2D_ARRAY_RTV {
    pub(super) MipSlice: u32,
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
    pub(super) PlaneSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2DMS_RTV {
    pub(super) UnusedField_NothingToDefine: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2DMS_ARRAY_RTV {
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX3D_RTV {
    pub(super) MipSlice: u32,
    pub(super) FirstWSlice: u32,
    pub(super) WSize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_DEPTH_STENCIL_VIEW_DESC {
    pub(super) Format: DXGI_FORMAT,
    pub(super) ViewDimension: D3D12_DSV_DIMENSION,
    pub(super) Flags: D3D12_DSV_FLAGS,
    pub(super) Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0,
}
pub(super) type D3D12_DSV_DIMENSION = i32;
pub(super) type D3D12_DSV_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
    pub(super) Texture1D: D3D12_TEX1D_DSV,
    pub(super) Texture1DArray: D3D12_TEX1D_ARRAY_DSV,
    pub(super) Texture2D: D3D12_TEX2D_DSV,
    pub(super) Texture2DArray: D3D12_TEX2D_ARRAY_DSV,
    pub(super) Texture2DMS: D3D12_TEX2DMS_DSV,
    pub(super) Texture2DMSArray: D3D12_TEX2DMS_ARRAY_DSV,
}
impl Default for D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX1D_DSV {
    pub(super) MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX1D_ARRAY_DSV {
    pub(super) MipSlice: u32,
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2D_DSV {
    pub(super) MipSlice: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2D_ARRAY_DSV {
    pub(super) MipSlice: u32,
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2DMS_DSV {
    pub(super) UnusedField_NothingToDefine: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEX2DMS_ARRAY_DSV {
    pub(super) FirstArraySlice: u32,
    pub(super) ArraySize: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_SAMPLER_DESC {
    pub(super) Filter: D3D12_FILTER,
    pub(super) AddressU: D3D12_TEXTURE_ADDRESS_MODE,
    pub(super) AddressV: D3D12_TEXTURE_ADDRESS_MODE,
    pub(super) AddressW: D3D12_TEXTURE_ADDRESS_MODE,
    pub(super) MipLODBias: f32,
    pub(super) MaxAnisotropy: u32,
    pub(super) ComparisonFunc: D3D12_COMPARISON_FUNC,
    pub(super) BorderColor: [f32; 4],
    pub(super) MinLOD: f32,
    pub(super) MaxLOD: f32,
}
pub(super) type D3D12_FILTER = i32;
pub(super) type D3D12_TEXTURE_ADDRESS_MODE = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_HEAP_PROPERTIES {
    pub(super) Type: D3D12_HEAP_TYPE,
    pub(super) CPUPageProperty: D3D12_CPU_PAGE_PROPERTY,
    pub(super) MemoryPoolPreference: D3D12_MEMORY_POOL,
    pub(super) CreationNodeMask: u32,
    pub(super) VisibleNodeMask: u32,
}
pub(super) type D3D12_HEAP_TYPE = i32;
pub(super) type D3D12_CPU_PAGE_PROPERTY = i32;
pub(super) type D3D12_MEMORY_POOL = i32;
pub(super) type D3D12_HEAP_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RESOURCE_DESC {
    pub(super) Dimension: D3D12_RESOURCE_DIMENSION,
    pub(super) Alignment: u64,
    pub(super) Width: u64,
    pub(super) Height: u32,
    pub(super) DepthOrArraySize: u16,
    pub(super) MipLevels: u16,
    pub(super) Format: DXGI_FORMAT,
    pub(super) SampleDesc: DXGI_SAMPLE_DESC,
    pub(super) Layout: D3D12_TEXTURE_LAYOUT,
    pub(super) Flags: D3D12_RESOURCE_FLAGS,
}
pub(super) type D3D12_RESOURCE_DIMENSION = i32;
pub(super) type D3D12_TEXTURE_LAYOUT = i32;
pub(super) type D3D12_RESOURCE_FLAGS = i32;
pub(super) type D3D12_RESOURCE_STATES = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_CLEAR_VALUE {
    pub(super) Format: DXGI_FORMAT,
    pub(super) Anonymous: D3D12_CLEAR_VALUE_0,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union D3D12_CLEAR_VALUE_0 {
    pub(super) Color: [f32; 4],
    pub(super) DepthStencil: D3D12_DEPTH_STENCIL_VALUE,
}
impl Default for D3D12_CLEAR_VALUE_0 {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_DEPTH_STENCIL_VALUE {
    pub(super) Depth: f32,
    pub(super) Stencil: u8,
}
pub(super) type D3D12_FENCE_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_TEXTURE_COPY_LOCATION {
    pub(super) pResource: *mut c_void,
    pub(super) Type: D3D12_TEXTURE_COPY_TYPE,
    pub(super) Anonymous: D3D12_TEXTURE_COPY_LOCATION_0,
}
pub(super) type D3D12_TEXTURE_COPY_TYPE = i32;
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union D3D12_TEXTURE_COPY_LOCATION_0 {
    pub(super) PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    pub(super) SubresourceIndex: u32,
}
impl Default for D3D12_TEXTURE_COPY_LOCATION_0 {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
    pub(super) Offset: u64,
    pub(super) Footprint: D3D12_SUBRESOURCE_FOOTPRINT,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_SUBRESOURCE_FOOTPRINT {
    pub(super) Format: DXGI_FORMAT,
    pub(super) Width: u32,
    pub(super) Height: u32,
    pub(super) Depth: u32,
    pub(super) RowPitch: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_BOX {
    pub(super) left: u32,
    pub(super) top: u32,
    pub(super) front: u32,
    pub(super) right: u32,
    pub(super) bottom: u32,
    pub(super) back: u32,
}
pub(super) type D3D_PRIMITIVE_TOPOLOGY = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_VIEWPORT {
    pub(super) TopLeftX: f32,
    pub(super) TopLeftY: f32,
    pub(super) Width: f32,
    pub(super) Height: f32,
    pub(super) MinDepth: f32,
    pub(super) MaxDepth: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RESOURCE_BARRIER {
    pub(super) Type: D3D12_RESOURCE_BARRIER_TYPE,
    pub(super) Flags: D3D12_RESOURCE_BARRIER_FLAGS,
    pub(super) Anonymous: D3D12_RESOURCE_BARRIER_0,
}
pub(super) type D3D12_RESOURCE_BARRIER_TYPE = i32;
pub(super) type D3D12_RESOURCE_BARRIER_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union D3D12_RESOURCE_BARRIER_0 {
    pub(super) Transition: D3D12_RESOURCE_TRANSITION_BARRIER,
    pub(super) Aliasing: D3D12_RESOURCE_ALIASING_BARRIER,
    pub(super) UAV: D3D12_RESOURCE_UAV_BARRIER,
}
impl Default for D3D12_RESOURCE_BARRIER_0 {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RESOURCE_TRANSITION_BARRIER {
    pub(super) pResource: *mut c_void,
    pub(super) Subresource: u32,
    pub(super) StateBefore: D3D12_RESOURCE_STATES,
    pub(super) StateAfter: D3D12_RESOURCE_STATES,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RESOURCE_ALIASING_BARRIER {
    pub(super) pResourceBefore: *mut c_void,
    pub(super) pResourceAfter: *mut c_void,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RESOURCE_UAV_BARRIER {
    pub(super) pResource: *mut c_void,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_GPU_DESCRIPTOR_HANDLE {
    pub(super) ptr: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_VERTEX_BUFFER_VIEW {
    pub(super) BufferLocation: u64,
    pub(super) SizeInBytes: u32,
    pub(super) StrideInBytes: u32,
}
pub(super) type D3D12_CLEAR_FLAGS = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_RANGE {
    pub(super) Begin: usize,
    pub(super) End: usize,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DXGI_SWAP_CHAIN_DESC1 {
    pub(super) Width: u32,
    pub(super) Height: u32,
    pub(super) Format: DXGI_FORMAT,
    pub(super) Stereo: i32,
    pub(super) SampleDesc: DXGI_SAMPLE_DESC,
    pub(super) BufferUsage: u32,
    pub(super) BufferCount: u32,
    pub(super) Scaling: DXGI_SCALING,
    pub(super) SwapEffect: DXGI_SWAP_EFFECT,
    pub(super) AlphaMode: DXGI_ALPHA_MODE,
    pub(super) Flags: u32,
}
pub(super) type DXGI_SCALING = i32;
pub(super) type DXGI_SWAP_EFFECT = i32;
pub(super) type DXGI_ALPHA_MODE = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DXGI_SWAP_CHAIN_FULLSCREEN_DESC {
    pub(super) RefreshRate: DXGI_RATIONAL,
    pub(super) ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER,
    pub(super) Scaling: DXGI_MODE_SCALING,
    pub(super) Windowed: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DXGI_RATIONAL {
    pub(super) Numerator: u32,
    pub(super) Denominator: u32,
}
pub(super) type DXGI_MODE_SCANLINE_ORDER = i32;
pub(super) type DXGI_MODE_SCALING = i32;
pub(super) type DXGI_MWA_FLAGS = i32;
pub(super) type DXGI_PRESENT = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_ROOT_SIGNATURE_DESC {
    pub(super) NumParameters: u32,
    pub(super) pParameters: *const D3D12_ROOT_PARAMETER,
    pub(super) NumStaticSamplers: u32,
    pub(super) pStaticSamplers: *const D3D12_STATIC_SAMPLER_DESC,
    pub(super) Flags: D3D12_ROOT_SIGNATURE_FLAGS,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_ROOT_PARAMETER {
    pub(super) ParameterType: D3D12_ROOT_PARAMETER_TYPE,
    pub(super) Anonymous: D3D12_ROOT_PARAMETER_0,
    pub(super) ShaderVisibility: D3D12_SHADER_VISIBILITY,
}
pub(super) type D3D12_ROOT_PARAMETER_TYPE = i32;
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) union D3D12_ROOT_PARAMETER_0 {
    pub(super) DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE,
    pub(super) Constants: D3D12_ROOT_CONSTANTS,
    pub(super) Descriptor: D3D12_ROOT_DESCRIPTOR,
}
impl Default for D3D12_ROOT_PARAMETER_0 {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_ROOT_DESCRIPTOR_TABLE {
    pub(super) NumDescriptorRanges: u32,
    pub(super) pDescriptorRanges: *const D3D12_DESCRIPTOR_RANGE,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_DESCRIPTOR_RANGE {
    pub(super) RangeType: D3D12_DESCRIPTOR_RANGE_TYPE,
    pub(super) NumDescriptors: u32,
    pub(super) BaseShaderRegister: u32,
    pub(super) RegisterSpace: u32,
    pub(super) OffsetInDescriptorsFromTableStart: u32,
}
pub(super) type D3D12_DESCRIPTOR_RANGE_TYPE = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_ROOT_CONSTANTS {
    pub(super) ShaderRegister: u32,
    pub(super) RegisterSpace: u32,
    pub(super) Num32BitValues: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_ROOT_DESCRIPTOR {
    pub(super) ShaderRegister: u32,
    pub(super) RegisterSpace: u32,
}
pub(super) type D3D12_SHADER_VISIBILITY = i32;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct D3D12_STATIC_SAMPLER_DESC {
    pub(super) Filter: D3D12_FILTER,
    pub(super) AddressU: D3D12_TEXTURE_ADDRESS_MODE,
    pub(super) AddressV: D3D12_TEXTURE_ADDRESS_MODE,
    pub(super) AddressW: D3D12_TEXTURE_ADDRESS_MODE,
    pub(super) MipLODBias: f32,
    pub(super) MaxAnisotropy: u32,
    pub(super) ComparisonFunc: D3D12_COMPARISON_FUNC,
    pub(super) BorderColor: D3D12_STATIC_BORDER_COLOR,
    pub(super) MinLOD: f32,
    pub(super) MaxLOD: f32,
    pub(super) ShaderRegister: u32,
    pub(super) RegisterSpace: u32,
    pub(super) ShaderVisibility: D3D12_SHADER_VISIBILITY,
}
pub(super) type D3D12_STATIC_BORDER_COLOR = i32;
pub(super) type D3D12_ROOT_SIGNATURE_FLAGS = i32;
pub(super) const ID3D12Device: GUID = GUID {
    data1: 412621297,
    data2: 7606,
    data3: 19287,
    data4: [190, 84, 24, 33, 51, 155, 133, 247],
};
pub(super) const ID3D12CommandQueue: GUID = GUID {
    data1: 248017062,
    data2: 23934,
    data3: 19490,
    data4: [140, 252, 91, 170, 224, 118, 22, 237],
};
pub(super) const ID3D12CommandAllocator: GUID = GUID {
    data1: 1627578084,
    data2: 44889,
    data3: 19209,
    data4: [185, 153, 180, 77, 115, 240, 155, 36],
};
pub(super) const ID3D12GraphicsCommandList: GUID = GUID {
    data1: 1528171791,
    data2: 44059,
    data3: 16773,
    data4: [139, 168, 179, 174, 66, 165, 164, 85],
};
pub(super) const ID3D12DescriptorHeap: GUID = GUID {
    data1: 2398832413,
    data2: 24940,
    data3: 20297,
    data4: [144, 247, 18, 123, 183, 99, 250, 81],
};
pub(super) const ID3D12Resource: GUID = GUID {
    data1: 1768178366,
    data2: 42798,
    data3: 16473,
    data4: [188, 121, 91, 92, 152, 4, 15, 173],
};
pub(super) const ID3D12Fence: GUID = GUID {
    data1: 175455695,
    data2: 50392,
    data3: 19345,
    data4: [173, 246, 190, 90, 96, 217, 90, 118],
};
pub(super) const ID3D12RootSignature: GUID = GUID {
    data1: 3309988710,
    data2: 29407,
    data3: 20200,
    data4: [139, 229, 169, 70, 161, 66, 146, 20],
};
pub(super) const ID3D12PipelineState: GUID = GUID {
    data1: 1985622259,
    data2: 63012,
    data3: 19567,
    data4: [168, 40, 172, 233, 72, 98, 36, 69],
};
pub(super) const IDXGIFactory4: GUID = GUID {
    data1: 466020866,
    data2: 61238,
    data3: 17999,
    data4: [191, 12, 33, 202, 57, 229, 22, 138],
};
pub(super) const IDXGISwapChain3: GUID = GUID {
    data1: 2497289179,
    data2: 61944,
    data3: 19120,
    data4: [178, 54, 125, 160, 23, 14, 218, 177],
};
pub(super) unsafe fn Device_CreateCommandQueue(
    a0: *mut ID3D12Device,
    a1: *const D3D12_COMMAND_QUEUE_DESC,
    a2: *const GUID,
    a3: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).CreateCommandQueue)(a0, a1, a2, a3) }
}
pub(super) unsafe fn Device_CreateCommandAllocator(
    a0: *mut ID3D12Device,
    a1: D3D12_COMMAND_LIST_TYPE,
    a2: *const GUID,
    a3: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).CreateCommandAllocator)(a0, a1, a2, a3) }
}
pub(super) unsafe fn Device_CreateGraphicsPipelineState(
    a0: *mut ID3D12Device,
    a1: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    a2: *const GUID,
    a3: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).CreateGraphicsPipelineState)(a0, a1, a2, a3) }
}
pub(super) unsafe fn Device_CreateCommandList(
    a0: *mut ID3D12Device,
    a1: u32,
    a2: D3D12_COMMAND_LIST_TYPE,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *const GUID,
    a6: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).CreateCommandList)(a0, a1, a2, a3, a4, a5, a6) }
}
pub(super) unsafe fn Device_CreateDescriptorHeap(
    a0: *mut ID3D12Device,
    a1: *const D3D12_DESCRIPTOR_HEAP_DESC,
    a2: *const GUID,
    a3: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).CreateDescriptorHeap)(a0, a1, a2, a3) }
}
pub(super) unsafe fn Device_GetDescriptorHandleIncrementSize(
    a0: *mut ID3D12Device,
    a1: D3D12_DESCRIPTOR_HEAP_TYPE,
) -> u32 {
    unsafe { ((*(*a0).vtbl).GetDescriptorHandleIncrementSize)(a0, a1) }
}
pub(super) unsafe fn Device_CreateRootSignature(
    a0: *mut ID3D12Device,
    a1: u32,
    a2: *const c_void,
    a3: usize,
    a4: *const GUID,
    a5: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).CreateRootSignature)(a0, a1, a2, a3, a4, a5) }
}
pub(super) unsafe fn Device_CreateShaderResourceView(
    a0: *mut ID3D12Device,
    a1: *mut c_void,
    a2: *const D3D12_SHADER_RESOURCE_VIEW_DESC,
    a3: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    unsafe { ((*(*a0).vtbl).CreateShaderResourceView)(a0, a1, a2, a3) }
}
pub(super) unsafe fn Device_CreateRenderTargetView(
    a0: *mut ID3D12Device,
    a1: *mut c_void,
    a2: *const D3D12_RENDER_TARGET_VIEW_DESC,
    a3: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    unsafe { ((*(*a0).vtbl).CreateRenderTargetView)(a0, a1, a2, a3) }
}
pub(super) unsafe fn Device_CreateDepthStencilView(
    a0: *mut ID3D12Device,
    a1: *mut c_void,
    a2: *const D3D12_DEPTH_STENCIL_VIEW_DESC,
    a3: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    unsafe { ((*(*a0).vtbl).CreateDepthStencilView)(a0, a1, a2, a3) }
}
pub(super) unsafe fn Device_CreateSampler(
    a0: *mut ID3D12Device,
    a1: *const D3D12_SAMPLER_DESC,
    a2: D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    unsafe { ((*(*a0).vtbl).CreateSampler)(a0, a1, a2) }
}
pub(super) unsafe fn Device_CreateCommittedResource(
    a0: *mut ID3D12Device,
    a1: *const D3D12_HEAP_PROPERTIES,
    a2: D3D12_HEAP_FLAGS,
    a3: *const D3D12_RESOURCE_DESC,
    a4: D3D12_RESOURCE_STATES,
    a5: *const D3D12_CLEAR_VALUE,
    a6: *const GUID,
    a7: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).CreateCommittedResource)(a0, a1, a2, a3, a4, a5, a6, a7) }
}
pub(super) unsafe fn Device_CreateFence(
    a0: *mut ID3D12Device,
    a1: u64,
    a2: D3D12_FENCE_FLAGS,
    a3: *const GUID,
    a4: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).CreateFence)(a0, a1, a2, a3, a4) }
}
pub(super) unsafe fn Device_GetDeviceRemovedReason(a0: *mut ID3D12Device) -> i32 {
    unsafe { ((*(*a0).vtbl).GetDeviceRemovedReason)(a0) }
}
pub(super) unsafe fn CommandQueue_ExecuteCommandLists(
    a0: *mut ID3D12CommandQueue,
    a1: u32,
    a2: *const *mut c_void,
) {
    unsafe { ((*(*a0).vtbl).ExecuteCommandLists)(a0, a1, a2) }
}
pub(super) unsafe fn CommandQueue_Signal(
    a0: *mut ID3D12CommandQueue,
    a1: *mut c_void,
    a2: u64,
) -> i32 {
    unsafe { ((*(*a0).vtbl).Signal)(a0, a1, a2) }
}
pub(super) unsafe fn CommandAllocator_Reset(a0: *mut ID3D12CommandAllocator) -> i32 {
    unsafe { ((*(*a0).vtbl).Reset)(a0) }
}
pub(super) unsafe fn GraphicsCommandList_Close(a0: *mut ID3D12GraphicsCommandList) -> i32 {
    unsafe { ((*(*a0).vtbl).Close)(a0) }
}
pub(super) unsafe fn GraphicsCommandList_Reset(
    a0: *mut ID3D12GraphicsCommandList,
    a1: *mut c_void,
    a2: *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).Reset)(a0, a1, a2) }
}
pub(super) unsafe fn GraphicsCommandList_DrawInstanced(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: u32,
    a3: u32,
    a4: u32,
) {
    unsafe { ((*(*a0).vtbl).DrawInstanced)(a0, a1, a2, a3, a4) }
}

#[repr(C)]
pub(super) struct D3D12_INDEX_BUFFER_VIEW {
    pub(super) BufferLocation: u64,
    pub(super) SizeInBytes: u32,
    pub(super) Format: i32,
}

pub(super) unsafe fn GraphicsCommandList_DrawIndexedInstanced(
    command: *mut ID3D12GraphicsCommandList,
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    base_vertex: i32,
    first_instance: u32,
) {
    unsafe {
        ((*(*command).vtbl).DrawIndexedInstanced)(
            command,
            index_count,
            instance_count,
            first_index,
            base_vertex,
            first_instance,
        );
    }
}

pub(super) unsafe fn GraphicsCommandList_IASetIndexBuffer(
    command: *mut ID3D12GraphicsCommandList,
    view: *const D3D12_INDEX_BUFFER_VIEW,
) {
    unsafe {
        ((*(*command).vtbl).IASetIndexBuffer)(command, view);
    }
}
pub(super) unsafe fn GraphicsCommandList_CopyBufferRegion(
    a0: *mut ID3D12GraphicsCommandList,
    a1: *mut c_void,
    a2: u64,
    a3: *mut c_void,
    a4: u64,
    a5: u64,
) {
    unsafe { ((*(*a0).vtbl).CopyBufferRegion)(a0, a1, a2, a3, a4, a5) }
}
pub(super) unsafe fn GraphicsCommandList_CopyTextureRegion(
    a0: *mut ID3D12GraphicsCommandList,
    a1: *const D3D12_TEXTURE_COPY_LOCATION,
    a2: u32,
    a3: u32,
    a4: u32,
    a5: *const D3D12_TEXTURE_COPY_LOCATION,
    a6: *const D3D12_BOX,
) {
    unsafe { ((*(*a0).vtbl).CopyTextureRegion)(a0, a1, a2, a3, a4, a5, a6) }
}
pub(super) unsafe fn GraphicsCommandList_IASetPrimitiveTopology(
    a0: *mut ID3D12GraphicsCommandList,
    a1: D3D_PRIMITIVE_TOPOLOGY,
) {
    unsafe { ((*(*a0).vtbl).IASetPrimitiveTopology)(a0, a1) }
}
pub(super) unsafe fn GraphicsCommandList_RSSetViewports(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: *const D3D12_VIEWPORT,
) {
    unsafe { ((*(*a0).vtbl).RSSetViewports)(a0, a1, a2) }
}
pub(super) unsafe fn GraphicsCommandList_RSSetScissorRects(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: *const RECT,
) {
    unsafe { ((*(*a0).vtbl).RSSetScissorRects)(a0, a1, a2) }
}
pub(super) unsafe fn GraphicsCommandList_SetPipelineState(
    a0: *mut ID3D12GraphicsCommandList,
    a1: *mut c_void,
) {
    unsafe { ((*(*a0).vtbl).SetPipelineState)(a0, a1) }
}
pub(super) unsafe fn GraphicsCommandList_ResourceBarrier(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: *const D3D12_RESOURCE_BARRIER,
) {
    unsafe { ((*(*a0).vtbl).ResourceBarrier)(a0, a1, a2) }
}
pub(super) unsafe fn GraphicsCommandList_SetDescriptorHeaps(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: *const *mut c_void,
) {
    unsafe { ((*(*a0).vtbl).SetDescriptorHeaps)(a0, a1, a2) }
}
pub(super) unsafe fn GraphicsCommandList_SetGraphicsRootSignature(
    a0: *mut ID3D12GraphicsCommandList,
    a1: *mut c_void,
) {
    unsafe { ((*(*a0).vtbl).SetGraphicsRootSignature)(a0, a1) }
}
pub(super) unsafe fn GraphicsCommandList_SetGraphicsRootDescriptorTable(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: D3D12_GPU_DESCRIPTOR_HANDLE,
) {
    unsafe { ((*(*a0).vtbl).SetGraphicsRootDescriptorTable)(a0, a1, a2) }
}
pub(super) unsafe fn GraphicsCommandList_SetGraphicsRootConstantBufferView(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: u64,
) {
    unsafe { ((*(*a0).vtbl).SetGraphicsRootConstantBufferView)(a0, a1, a2) }
}
pub(super) unsafe fn GraphicsCommandList_SetGraphicsRootShaderResourceView(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: u64,
) {
    unsafe { ((*(*a0).vtbl).SetGraphicsRootShaderResourceView)(a0, a1, a2) }
}

pub(super) unsafe fn GraphicsCommandList_SetGraphicsRoot32BitConstants(
    command: *mut ID3D12GraphicsCommandList,
    parameter: u32,
    count: u32,
    values: *const c_void,
    offset: u32,
) {
    unsafe {
        ((*(*command).vtbl).SetGraphicsRoot32BitConstants)(
            command, parameter, count, values, offset,
        );
    }
}
pub(super) unsafe fn GraphicsCommandList_IASetVertexBuffers(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: u32,
    a3: *const D3D12_VERTEX_BUFFER_VIEW,
) {
    unsafe { ((*(*a0).vtbl).IASetVertexBuffers)(a0, a1, a2, a3) }
}
pub(super) unsafe fn GraphicsCommandList_OMSetRenderTargets(
    a0: *mut ID3D12GraphicsCommandList,
    a1: u32,
    a2: *const D3D12_CPU_DESCRIPTOR_HANDLE,
    a3: i32,
    a4: *const D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    unsafe { ((*(*a0).vtbl).OMSetRenderTargets)(a0, a1, a2, a3, a4) }
}
pub(super) unsafe fn GraphicsCommandList_ClearDepthStencilView(
    a0: *mut ID3D12GraphicsCommandList,
    a1: D3D12_CPU_DESCRIPTOR_HANDLE,
    a2: D3D12_CLEAR_FLAGS,
    a3: f32,
    a4: u8,
    a5: u32,
    a6: *const RECT,
) {
    unsafe { ((*(*a0).vtbl).ClearDepthStencilView)(a0, a1, a2, a3, a4, a5, a6) }
}
pub(super) unsafe fn GraphicsCommandList_ClearRenderTargetView(
    a0: *mut ID3D12GraphicsCommandList,
    a1: D3D12_CPU_DESCRIPTOR_HANDLE,
    a2: *const f32,
    a3: u32,
    a4: *const RECT,
) {
    unsafe { ((*(*a0).vtbl).ClearRenderTargetView)(a0, a1, a2, a3, a4) }
}
pub(super) unsafe fn Resource_Map(
    a0: *mut ID3D12Resource,
    a1: u32,
    a2: *const D3D12_RANGE,
    a3: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).Map)(a0, a1, a2, a3) }
}
pub(super) unsafe fn Resource_Unmap(a0: *mut ID3D12Resource, a1: u32, a2: *const D3D12_RANGE) {
    unsafe { ((*(*a0).vtbl).Unmap)(a0, a1, a2) }
}
pub(super) unsafe fn Resource_GetGPUVirtualAddress(a0: *mut ID3D12Resource) -> u64 {
    unsafe { ((*(*a0).vtbl).GetGPUVirtualAddress)(a0) }
}
pub(super) unsafe fn DescriptorHeap_GetCPUDescriptorHandleForHeapStart(
    a0: *mut ID3D12DescriptorHeap,
    a1: *mut D3D12_CPU_DESCRIPTOR_HANDLE,
) {
    unsafe { ((*(*a0).vtbl).GetCPUDescriptorHandleForHeapStart)(a0, a1) }
}
pub(super) unsafe fn DescriptorHeap_GetGPUDescriptorHandleForHeapStart(
    a0: *mut ID3D12DescriptorHeap,
    a1: *mut D3D12_GPU_DESCRIPTOR_HANDLE,
) {
    unsafe { ((*(*a0).vtbl).GetGPUDescriptorHandleForHeapStart)(a0, a1) }
}
pub(super) unsafe fn Fence_GetCompletedValue(a0: *mut ID3D12Fence) -> u64 {
    unsafe { ((*(*a0).vtbl).GetCompletedValue)(a0) }
}
pub(super) unsafe fn Fence_SetEventOnCompletion(a0: *mut ID3D12Fence, a1: u64, a2: HANDLE) -> i32 {
    unsafe { ((*(*a0).vtbl).SetEventOnCompletion)(a0, a1, a2) }
}
pub(super) unsafe fn Factory4_CreateSwapChainForHwnd(
    a0: *mut IDXGIFactory4,
    a1: *mut c_void,
    a2: HWND,
    a3: *const DXGI_SWAP_CHAIN_DESC1,
    a4: *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    a5: *mut c_void,
    a6: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).base.base.CreateSwapChainForHwnd)(a0.cast(), a1, a2, a3, a4, a5, a6) }
}
// IDXGIFactory2::CreateSwapChainForComposition, following CreateSwapChainForCoreWindow.
pub(super) unsafe fn Factory4_CreateSwapChainForComposition(
    a0: *mut IDXGIFactory4,
    a1: *mut c_void,
    a2: *const DXGI_SWAP_CHAIN_DESC1,
    a3: *mut c_void,
    a4: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).base.base.CreateSwapChainForComposition)(a0.cast(), a1, a2, a3, a4) }
}
// IDCompositionVisual::SetContent.
pub(super) unsafe fn Visual_SetContent(a0: *mut c_void, a1: *mut c_void) -> i32 {
    unsafe {
        let this = a0.cast::<IDCompositionVisual>();
        ((*(*this).vtbl).SetContent)(this, a1)
    }
}
// IDCompositionDevice::Commit.
pub(super) unsafe fn CompositionDevice_Commit(a0: *mut c_void) -> i32 {
    unsafe {
        let this = a0.cast::<IDCompositionDevice>();
        ((*(*this).vtbl).Commit)(this)
    }
}
pub(super) unsafe fn Factory4_MakeWindowAssociation(
    a0: *mut IDXGIFactory4,
    a1: HWND,
    a2: DXGI_MWA_FLAGS,
) -> i32 {
    unsafe { ((*(*a0).vtbl).base.base.base.base.MakeWindowAssociation)(a0.cast(), a1, a2) }
}
pub(super) unsafe fn SwapChain3_GetBuffer(
    a0: *mut IDXGISwapChain3,
    a1: u32,
    a2: *const GUID,
    a3: *mut *mut c_void,
) -> i32 {
    unsafe { ((*(*a0).vtbl).base.base.base.GetBuffer)(a0.cast(), a1, a2, a3) }
}
pub(super) unsafe fn SwapChain3_ResizeBuffers(
    a0: *mut IDXGISwapChain3,
    a1: u32,
    a2: u32,
    a3: u32,
    a4: DXGI_FORMAT,
    a5: u32,
) -> i32 {
    unsafe { ((*(*a0).vtbl).base.base.base.ResizeBuffers)(a0.cast(), a1, a2, a3, a4, a5) }
}
pub(super) unsafe fn SwapChain3_Present(
    a0: *mut IDXGISwapChain3,
    a1: u32,
    a2: DXGI_PRESENT,
) -> i32 {
    unsafe { ((*(*a0).vtbl).base.base.base.Present)(a0.cast(), a1, a2) }
}
pub(super) unsafe fn SwapChain3_GetCurrentBackBufferIndex(a0: *mut IDXGISwapChain3) -> u32 {
    unsafe { ((*(*a0).vtbl).GetCurrentBackBufferIndex)(a0) }
}
pub(super) unsafe fn Blob_GetBufferPointer(a0: *mut ID3DBlob) -> *mut c_void {
    unsafe { ((*(*a0).vtbl).GetBufferPointer)(a0) }
}
pub(super) unsafe fn Blob_GetBufferSize(a0: *mut ID3DBlob) -> usize {
    unsafe { ((*(*a0).vtbl).GetBufferSize)(a0) }
}

#[link(name = "d3d12")]
unsafe extern "system" {
    pub(super) fn D3D12CreateDevice(
        adapter: *mut c_void,
        level: u32,
        iid: *const GUID,
        result: *mut *mut c_void,
    ) -> i32;
    pub(super) fn D3D12SerializeRootSignature(
        desc: *const D3D12_ROOT_SIGNATURE_DESC,
        version: i32,
        blob: *mut *mut c_void,
        error: *mut *mut c_void,
    ) -> i32;
}
#[link(name = "dxgi")]
unsafe extern "system" {
    pub(super) fn CreateDXGIFactory2(flags: u32, iid: *const GUID, result: *mut *mut c_void)
    -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    pub(super) fn CreateEventW(
        attributes: *const c_void,
        manual: i32,
        initial: i32,
        name: *const u16,
    ) -> HANDLE;
    pub(super) fn WaitForSingleObject(event: HANDLE, timeout: u32) -> u32;
    pub(super) fn CloseHandle(event: HANDLE) -> BOOL;
}
pub(super) type Compile = unsafe extern "system" fn(
    *const c_void,
    usize,
    *const c_char,
    *const c_void,
    *mut c_void,
    *const c_char,
    *const c_char,
    u32,
    u32,
    *mut *mut c_void,
    *mut *mut c_void,
) -> i32;

#[repr(C)]
pub(super) struct DXGI_ADAPTER_DESC1 {
    pub(super) Description: [u16; 128],
    pub(super) VendorId: u32,
    pub(super) DeviceId: u32,
    pub(super) SubSysId: u32,
    pub(super) Revision: u32,
    pub(super) DedicatedVideoMemory: usize,
    pub(super) DedicatedSystemMemory: usize,
    pub(super) SharedSystemMemory: usize,
    pub(super) AdapterLuid: [u32; 2],
    pub(super) Flags: u32,
}

pub(super) unsafe fn Factory_EnumAdapters1(
    factory: *mut IDXGIFactory4,
    index: u32,
    out: *mut *mut c_void,
) -> HRESULT {
    unsafe { ((*(*factory).vtbl).base.base.base.EnumAdapters1)(factory.cast(), index, out) }
}

pub(super) unsafe fn Adapter_GetDesc1(
    adapter: *mut IDXGIAdapter1,
    desc: *mut DXGI_ADAPTER_DESC1,
) -> HRESULT {
    unsafe { ((*(*adapter).vtbl).GetDesc1)(adapter, desc) }
}

// IDXGIFactory4::EnumWarpAdapter, following EnumAdapterByLuid.
pub(super) unsafe fn Factory4_EnumWarpAdapter(
    factory: *mut IDXGIFactory4,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    unsafe { ((*(*factory).vtbl).EnumWarpAdapter)(factory, iid, out) }
}
pub(super) const IDXGIAdapter: GUID = GUID {
    data1: 0x2411e7e1,
    data2: 0x12ac,
    data3: 0x4ccf,
    data4: [0xbd, 0x14, 0x97, 0x98, 0xe8, 0x53, 0x4d, 0xc0],
};

#[cfg(feature = "window")]
#[link(name = "user32")]
unsafe extern "system" {
    pub(super) fn GetClientRect(window: HWND, rect: *mut RECT) -> BOOL;
}
