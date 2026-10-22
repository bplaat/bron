/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Direct3D 12 through local COM/Win32 declarations.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::rc::Rc;

#[cfg(feature = "window")]
pub(crate) use surface::{Factory, Surface};

use crate::platform::headers::*;
use crate::*;

mod headers;
#[cfg(feature = "window")]
mod surface;

struct InitError {
    error: Error,
    fallback: bool,
}

impl InitError {
    const fn unavailable(error: Error) -> Self {
        Self {
            error,
            fallback: true,
        }
    }

    const fn fatal(error: Error) -> Self {
        Self {
            error,
            fallback: false,
        }
    }

    const fn device(error: Error, status: i32) -> Self {
        if matches!(
            status as u32,
            0x80004002 // E_NOINTERFACE
                | 0x887a0002 // DXGI_ERROR_NOT_FOUND
                | 0x887a0004 // DXGI_ERROR_UNSUPPORTED
                | 0x887e0001 // D3D12_ERROR_ADAPTER_NOT_FOUND
                | 0x887e0002 // D3D12_ERROR_DRIVER_VERSION_MISMATCH
        ) {
            Self::unavailable(error)
        } else {
            Self::fatal(error)
        }
    }
}

fn check(result: i32) -> Result<()> {
    if result >= 0 {
        Ok(())
    } else {
        Err(Error(format!("Direct3D error 0x{:08x}", result as u32)))
    }
}

struct Com(*mut c_void);

impl Com {
    fn create(create: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Result<Self> {
        let mut value = ptr::null_mut();
        let result = create(&mut value);
        let owned = (!value.is_null()).then(|| Self(value));
        check(result)?;
        owned.ok_or_else(|| Error("COM returned null".into()))
    }

    #[cfg(feature = "window")]
    fn query(&self, iid: &GUID) -> Result<Self> {
        Self::create(|out| unsafe { (self.unknown().QueryInterface)(self.0, iid, out) })
    }

    fn unknown(&self) -> &IUnknownVtbl {
        unsafe { &**self.0.cast::<*const IUnknownVtbl>() }
    }
}

impl Clone for Com {
    fn clone(&self) -> Self {
        unsafe { (self.unknown().AddRef)(self.0) };
        Self(self.0)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { (self.unknown().Release)(self.0) };
    }
}

struct Event(HANDLE);

impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

const fn format(value: TextureFormat) -> i32 {
    match value {
        TextureFormat::Rgba8Unorm => 28,
        TextureFormat::Rgba8UnormSrgb => 29,
        TextureFormat::Bgra8Unorm => 87,
        TextureFormat::Bgra8UnormSrgb => 91,
        TextureFormat::Depth32Float => 40,
    }
}

pub(crate) struct Buffer {
    object: Com,
    state: Cell<i32>,
}

pub(crate) struct Texture {
    object: Com,
    state: Cell<i32>,
    format: TextureFormat,
    mips: u32,
    layers: u32,
    index: Option<u32>,
}

pub(crate) struct Sampler {
    desc: D3D12_SAMPLER_DESC,
}

pub(crate) struct Pipeline {
    object: Com,
    root: Com,
}

struct Heap {
    object: Com,
    cpu: D3D12_CPU_DESCRIPTOR_HANDLE,
    gpu: D3D12_GPU_DESCRIPTOR_HANDLE,
    stride: u32,
}

impl Heap {
    const fn cpu(&self, index: u32) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: self.cpu.ptr + index as usize * self.stride as usize,
        }
    }

    fn gpu(&self, index: u32) -> D3D12_GPU_DESCRIPTOR_HANDLE {
        D3D12_GPU_DESCRIPTOR_HANDLE {
            ptr: self.gpu.ptr + u64::from(index) * u64::from(self.stride),
        }
    }
}

struct Frame {
    allocator: Com,
    list: Com,
    resources: Heap,
    samplers: Heap,
    colors: Heap,
    depths: Heap,
    fence: u64,
    uploads: Vec<Upload>,
    passes: Vec<Pass>,
    staging: Vec<Com>,
    staging_capacity: u64,
}

pub(crate) struct Context {
    device: Com,
    queue: Com,
    #[cfg(feature = "window")]
    factory: Com,
    fence: Com,
    event: Event,
    root: Com,
    sampler_indices: Com,
    frames: RefCell<Vec<Frame>>,
    slot: Cell<usize>,
    serial: Cell<u64>,
    #[cfg(feature = "window")]
    acquired: Cell<bool>,
}

impl Context {
    pub(super) fn new() -> Result<Rc<Self>> {
        match Self::with_driver(false) {
            Ok(context) => Ok(context),
            Err(error) if !error.fallback => Err(error.error),
            Err(hardware) => Self::with_driver(true).map_err(|warp| {
                Error(format!(
                    "No Direct3D 12 backend is available (hardware: {}; WARP: {})",
                    hardware.error, warp.error
                ))
            }),
        }
    }

    fn with_driver(warp: bool) -> std::result::Result<Rc<Self>, InitError> {
        unsafe {
            let factory = Com::create(|out| CreateDXGIFactory2(0, &IDXGIFactory4, out))
                .map_err(InitError::fatal)?;
            let device = if warp {
                let adapter =
                    Com::create(|out| Factory4_EnumWarpAdapter(factory.0, &IDXGIAdapter, out))
                        .map_err(InitError::fatal)?;
                Self::create_device(&adapter)?
            } else {
                Self::hardware_device(&factory)?
            };
            (|| -> Result<Rc<Self>> {
                let desc = D3D12_COMMAND_QUEUE_DESC::default();
                let queue = Com::create(|out| {
                    Device_CreateCommandQueue(device.0, &desc, &ID3D12CommandQueue, out)
                })?;
                let fence =
                    Com::create(|out| Device_CreateFence(device.0, 0, 0, &ID3D12Fence, out))?;
                let event = CreateEventW(ptr::null(), 0, 0, ptr::null());
                if event.is_null() {
                    return Err(Error("CreateEventW failed".into()));
                }
                let event = Event(event);
                let root = Self::root_signature(&device)
                    .map_err(|error| Error(format!("D3D12 root signature: {error}")))?;
                let sampler_indices = Self::resource(&device, &Self::buffer_desc(4), true)?;
                let mut data = ptr::null_mut();
                check(Resource_Map(
                    sampler_indices.0,
                    0,
                    &D3D12_RANGE::default(),
                    &mut data,
                ))?;
                // Each draw binds one sampler, so its shader index is always zero.
                data.cast::<u32>().write(0);
                Resource_Unmap(sampler_indices.0, 0, ptr::null());
                let context = Rc::new(Self {
                    device,
                    queue,
                    #[cfg(feature = "window")]
                    factory,
                    fence,
                    event,
                    root,
                    sampler_indices,
                    frames: RefCell::new(Vec::new()),
                    slot: Cell::new(0),
                    serial: Cell::new(0),
                    #[cfg(feature = "window")]
                    acquired: Cell::new(false),
                });
                for _ in 0..2 {
                    context.frames.borrow_mut().push(context.frame()?);
                }
                Ok(context)
            })()
            .map_err(InitError::fatal)
        }
    }

    fn create_device(adapter: &Com) -> std::result::Result<Com, InitError> {
        let mut raw_device = ptr::null_mut();
        let status = unsafe {
            D3D12CreateDevice(
                adapter.0,
                0xb000, // D3D_FEATURE_LEVEL_11_0, including resource binding tier 1.
                &ID3D12Device,
                &mut raw_device,
            )
        };
        let device = (!raw_device.is_null()).then(|| Com(raw_device));
        if status < 0 {
            return Err(InitError::device(
                Error(format!("D3D12CreateDevice: 0x{:08x}", status as u32)),
                status,
            ));
        }
        device.ok_or_else(|| InitError::fatal(Error("D3D12 returned null".into())))
    }

    fn hardware_device(factory: &Com) -> std::result::Result<Com, InitError> {
        let mut failures = Vec::new();
        for index in 0.. {
            let mut raw_adapter = ptr::null_mut();
            let status = unsafe { Factory_EnumAdapters1(factory.0, index, &mut raw_adapter) };
            let adapter = (!raw_adapter.is_null()).then(|| Com(raw_adapter));
            if status as u32 == 0x887a0002 {
                // DXGI_ERROR_NOT_FOUND: enumeration is finished.
                break;
            }
            check(status).map_err(InitError::fatal)?;
            let adapter =
                adapter.ok_or_else(|| InitError::fatal(Error("DXGI returned null".into())))?;
            let mut desc = std::mem::MaybeUninit::<DXGI_ADAPTER_DESC1>::uninit();
            check(unsafe { Adapter_GetDesc1(adapter.0, desc.as_mut_ptr()) })
                .map_err(InitError::fatal)?;
            let desc = unsafe { desc.assume_init() };
            if desc.Flags & 2 != 0 {
                // DXGI_ADAPTER_FLAG_SOFTWARE: use the explicit WARP fallback.
                continue;
            }
            match Self::create_device(&adapter) {
                Ok(device) => return Ok(device),
                Err(error) if !error.fallback => return Err(error),
                Err(error) => {
                    let length = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
                    failures.push(format!(
                        "{}: {}",
                        String::from_utf16_lossy(&desc.Description[..length]),
                        error.error
                    ));
                }
            }
        }
        Err(InitError::unavailable(Error(if failures.is_empty() {
            "No hardware adapters found".into()
        } else {
            failures.join("; ")
        })))
    }

    fn root_signature(device: &Com) -> Result<Com> {
        unsafe {
            let texture = D3D12_DESCRIPTOR_RANGE {
                RangeType: 0,
                NumDescriptors: 1,
                BaseShaderRegister: 1,
                ..Default::default()
            };
            let sampler = D3D12_DESCRIPTOR_RANGE {
                RangeType: 3,
                NumDescriptors: 1,
                BaseShaderRegister: 0,
                ..Default::default()
            };
            let parameters = [
                D3D12_ROOT_PARAMETER {
                    ParameterType: 2,
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        Descriptor: D3D12_ROOT_DESCRIPTOR {
                            ShaderRegister: 0,
                            RegisterSpace: 0,
                        },
                    },
                    ShaderVisibility: 0,
                },
                D3D12_ROOT_PARAMETER {
                    ParameterType: 0,
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                            NumDescriptorRanges: 1,
                            pDescriptorRanges: &texture,
                        },
                    },
                    ShaderVisibility: 5,
                },
                D3D12_ROOT_PARAMETER {
                    ParameterType: 3,
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        Descriptor: D3D12_ROOT_DESCRIPTOR {
                            ShaderRegister: 3,
                            RegisterSpace: 0,
                        },
                    },
                    ShaderVisibility: 1,
                },
                D3D12_ROOT_PARAMETER {
                    ParameterType: 0,
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                            NumDescriptorRanges: 1,
                            pDescriptorRanges: &sampler,
                        },
                    },
                    ShaderVisibility: 5,
                },
                D3D12_ROOT_PARAMETER {
                    ParameterType: 3,
                    Anonymous: D3D12_ROOT_PARAMETER_0 {
                        Descriptor: D3D12_ROOT_DESCRIPTOR {
                            ShaderRegister: 4,
                            RegisterSpace: 0,
                        },
                    },
                    ShaderVisibility: 5,
                },
            ];
            let desc = D3D12_ROOT_SIGNATURE_DESC {
                NumParameters: parameters.len() as u32,
                pParameters: parameters.as_ptr(),
                Flags: 1,
                ..Default::default()
            };
            let mut errors = ptr::null_mut();
            let blob = Com::create(|out| D3D12SerializeRootSignature(&desc, 1, out, &mut errors));
            if !errors.is_null() {
                drop(Com(errors));
            }
            let blob = blob?;
            Com::create(|out| {
                Device_CreateRootSignature(
                    device.0,
                    0,
                    Blob_GetBufferPointer(blob.0),
                    Blob_GetBufferSize(blob.0),
                    &ID3D12RootSignature,
                    out,
                )
            })
        }
    }

    fn initialize_descriptors(&self, frame: &Frame, index: u32) {
        let texture = D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: 28,
            ViewDimension: 4,
            Shader4ComponentMapping: 5768,
            Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D12_TEX2D_SRV {
                    MipLevels: 1,
                    ..Default::default()
                },
            },
        };
        let sampler = D3D12_SAMPLER_DESC {
            AddressU: 3,
            AddressV: 3,
            AddressW: 3,
            MaxAnisotropy: 1,
            ComparisonFunc: 8,
            MaxLOD: f32::MAX,
            ..Default::default()
        };
        unsafe {
            Device_CreateShaderResourceView(
                self.device.0,
                ptr::null_mut(),
                &texture,
                frame.resources.cpu(index),
            );
            Device_CreateSampler(self.device.0, &sampler, frame.samplers.cpu(index));
        }
    }

    fn heap(&self, kind: i32, count: u32, visible: bool) -> Result<Heap> {
        unsafe {
            let desc = D3D12_DESCRIPTOR_HEAP_DESC {
                Type: kind,
                NumDescriptors: count,
                Flags: i32::from(visible),
                ..Default::default()
            };
            let object = Com::create(|out| {
                Device_CreateDescriptorHeap(self.device.0, &desc, &ID3D12DescriptorHeap, out)
            })?;
            let mut cpu = D3D12_CPU_DESCRIPTOR_HANDLE::default();
            let mut gpu = D3D12_GPU_DESCRIPTOR_HANDLE::default();
            DescriptorHeap_GetCPUDescriptorHandleForHeapStart(object.0, &mut cpu);
            if visible {
                DescriptorHeap_GetGPUDescriptorHandleForHeapStart(object.0, &mut gpu);
            }
            Ok(Heap {
                object,
                cpu,
                gpu,
                stride: Device_GetDescriptorHandleIncrementSize(self.device.0, kind),
            })
        }
    }

    fn frame(&self) -> Result<Frame> {
        unsafe {
            let allocator = Com::create(|out| {
                Device_CreateCommandAllocator(self.device.0, 0, &ID3D12CommandAllocator, out)
            })?;
            let list = Com::create(|out| {
                Device_CreateCommandList(
                    self.device.0,
                    0,
                    0,
                    allocator.0,
                    ptr::null_mut(),
                    &ID3D12GraphicsCommandList,
                    out,
                )
            })?;
            check(GraphicsCommandList_Close(list.0))?;
            Ok(Frame {
                allocator,
                list,
                resources: self.heap(0, 257, true)?,
                samplers: self.heap(1, 257, true)?,
                colors: self.heap(2, 16, false)?,
                depths: self.heap(3, 16, false)?,
                fence: 0,
                uploads: Vec::new(),
                passes: Vec::new(),
                staging: Vec::new(),
                staging_capacity: 0,
            })
        }
    }

    fn wait_value(&self, value: u64) -> Result<()> {
        unsafe {
            let completed = Fence_GetCompletedValue(self.fence.0);
            if completed == u64::MAX {
                return check(Device_GetDeviceRemovedReason(self.device.0));
            }
            if completed < value {
                check(Fence_SetEventOnCompletion(
                    self.fence.0,
                    value,
                    self.event.0,
                ))?;
                if WaitForSingleObject(self.event.0, u32::MAX) != 0 {
                    return Err(Error("GPU fence wait failed".into()));
                }
            }
            Ok(())
        }
    }

    fn wait(&self, frame: &mut Frame) -> Result<()> {
        self.wait_value(frame.fence)?;
        frame.uploads.clear();
        frame.passes.clear();
        Ok(())
    }

    fn signal(&self) -> Result<u64> {
        let value = self.serial.get() + 1;
        self.serial.set(value);
        unsafe {
            check(CommandQueue_Signal(self.queue.0, self.fence.0, value))?;
        }
        Ok(value)
    }

    fn buffer_desc(size: u64) -> D3D12_RESOURCE_DESC {
        D3D12_RESOURCE_DESC {
            Dimension: 1,
            Width: size.div_ceil(256) * 256,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: 1,
            ..Default::default()
        }
    }

    fn resource(device: &Com, desc: &D3D12_RESOURCE_DESC, upload: bool) -> Result<Com> {
        let heap = D3D12_HEAP_PROPERTIES {
            Type: if upload { 2 } else { 1 },
            CreationNodeMask: 1,
            VisibleNodeMask: 1,
            ..Default::default()
        };
        unsafe {
            Com::create(|out| {
                Device_CreateCommittedResource(
                    device.0,
                    &heap,
                    0,
                    desc,
                    if upload { 2755 } else { 0 },
                    ptr::null(),
                    &ID3D12Resource,
                    out,
                )
            })
        }
    }

    pub(super) fn buffer(&self, size: u64, _: BufferUsages) -> Result<Buffer> {
        Ok(Buffer {
            object: Self::resource(&self.device, &Self::buffer_desc(size), false)?,
            state: Cell::new(0),
        })
    }

    pub(super) fn texture(&self, desc: &TextureDescriptor<'_>) -> Result<Texture> {
        let native = D3D12_RESOURCE_DESC {
            Dimension: 3,
            Width: u64::from(desc.size.width),
            Height: desc.size.height,
            DepthOrArraySize: u16::try_from(desc.size.depth_or_array_layers)
                .map_err(|_| Error("Too many texture layers".into()))?,
            MipLevels: desc.mip_level_count as u16,
            Format: format(desc.format),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Flags: if desc.format == TextureFormat::Depth32Float {
                2 // D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL
            } else if desc.usage.contains(TextureUsages::RENDER_ATTACHMENT) {
                1 // D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
            } else {
                0
            },
            ..Default::default()
        };
        Ok(Texture {
            object: Self::resource(&self.device, &native, false)?,
            state: Cell::new(0),
            format: desc.format,
            mips: desc.mip_level_count,
            layers: desc.size.depth_or_array_layers,
            index: None,
        })
    }

    pub(super) fn sampler(&self, desc: &SamplerDescriptor<'_>) -> Result<Sampler> {
        Ok(Sampler {
            desc: D3D12_SAMPLER_DESC {
                Filter: (if desc.min_filter == FilterMode::Linear {
                    16
                } else {
                    0
                }) | (if desc.mag_filter == FilterMode::Linear {
                    4
                } else {
                    0
                }) | i32::from(desc.mipmap_filter == FilterMode::Linear),
                AddressU: if desc.address_mode_u == AddressMode::Repeat {
                    1
                } else {
                    3
                },
                AddressV: if desc.address_mode_v == AddressMode::Repeat {
                    1
                } else {
                    3
                },
                AddressW: 3,
                MaxAnisotropy: 1,
                ComparisonFunc: 8,
                MaxLOD: f32::MAX,
                ..Default::default()
            },
        })
    }

    pub(super) fn pipeline(&self, desc: &RenderPipelineDescriptor<'_>) -> Result<Pipeline> {
        let fragment = desc.fragment.as_ref().expect("fragment stage validated");
        let color = fragment.targets[0].expect("color target validated");
        let vertex = desc
            .vertex
            .module
            .binary_for("dx12", desc.vertex.entry_point)
            .ok_or_else(|| Error("D3D12 vertex entry point has no bytecode".into()))?;
        let pixel = fragment
            .module
            .binary_for("dx12", fragment.entry_point)
            .ok_or_else(|| Error("D3D12 fragment entry point has no bytecode".into()))?;
        let layout = desc.vertex.buffers.first().and_then(Option::as_ref);
        let input = layout
            .into_iter()
            .flat_map(|layout| layout.attributes)
            .map(|attribute| D3D12_INPUT_ELEMENT_DESC {
                SemanticName: c"LOC".as_ptr().cast(),
                SemanticIndex: attribute.shader_location,
                Format: match attribute.format {
                    VertexFormat::Float32x2 => 16,
                    VertexFormat::Float32x3 => 6,
                    VertexFormat::Uint32x2 => 17,
                },
                AlignedByteOffset: attribute.offset as u32,
                InputSlotClass: i32::from(
                    layout.is_some_and(|layout| layout.step_mode == VertexStepMode::Instance),
                ),
                InstanceDataStepRate: u32::from(
                    layout.is_some_and(|layout| layout.step_mode == VertexStepMode::Instance),
                ),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let blend = D3D12_RENDER_TARGET_BLEND_DESC {
            BlendEnable: i32::from(color.blend.is_some()),
            SrcBlend: 5,
            DestBlend: 6,
            BlendOp: 1,
            SrcBlendAlpha: 2,
            DestBlendAlpha: 6,
            BlendOpAlpha: 1,
            LogicOp: 5,
            RenderTargetWriteMask: 15,
            ..Default::default()
        };
        let depth = desc.depth_stencil;
        unsafe {
            let native = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
                pRootSignature: self.root.0,
                VS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: vertex.as_ptr().cast(),
                    BytecodeLength: vertex.len(),
                },
                PS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: pixel.as_ptr().cast(),
                    BytecodeLength: pixel.len(),
                },
                BlendState: D3D12_BLEND_DESC {
                    RenderTarget: [blend; 8],
                    ..Default::default()
                },
                SampleMask: u32::MAX,
                RasterizerState: D3D12_RASTERIZER_DESC {
                    FillMode: 3,
                    CullMode: if desc.primitive.cull_mode.is_some() {
                        3
                    } else {
                        1
                    },
                    FrontCounterClockwise: 1,
                    DepthClipEnable: 1,
                    ..Default::default()
                },
                DepthStencilState: D3D12_DEPTH_STENCIL_DESC {
                    DepthEnable: i32::from(depth.is_some()),
                    DepthWriteMask: i32::from(
                        depth.is_some_and(|depth| depth.depth_write_enabled.unwrap_or(false)),
                    ),
                    DepthFunc: if depth
                        .is_some_and(|depth| depth.depth_compare == Some(CompareFunction::Always))
                    {
                        8
                    } else {
                        4
                    },
                    FrontFace: D3D12_DEPTH_STENCILOP_DESC {
                        StencilFailOp: 1,
                        StencilDepthFailOp: 1,
                        StencilPassOp: 1,
                        StencilFunc: 8,
                    },
                    BackFace: D3D12_DEPTH_STENCILOP_DESC {
                        StencilFailOp: 1,
                        StencilDepthFailOp: 1,
                        StencilPassOp: 1,
                        StencilFunc: 8,
                    },
                    ..Default::default()
                },
                InputLayout: D3D12_INPUT_LAYOUT_DESC {
                    pInputElementDescs: input.as_ptr(),
                    NumElements: input.len() as u32,
                },
                PrimitiveTopologyType: 3,
                NumRenderTargets: 1,
                RTVFormats: [format(color.format), 0, 0, 0, 0, 0, 0, 0],
                DSVFormat: if depth.is_some() { 40 } else { 0 },
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                ..Default::default()
            };
            Ok(Pipeline {
                object: Com::create(|out| {
                    Device_CreateGraphicsPipelineState(
                        self.device.0,
                        &native,
                        &ID3D12PipelineState,
                        out,
                    )
                })?,
                root: self.root.clone(),
            })
        }
    }

    unsafe fn transition(list: &Com, object: &Com, state: &Cell<i32>, next: i32) {
        let previous = state.replace(next);
        if previous == next {
            return;
        }
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: 0,
            Flags: 0,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: D3D12_RESOURCE_TRANSITION_BARRIER {
                    pResource: object.0,
                    Subresource: u32::MAX,
                    StateBefore: previous,
                    StateAfter: next,
                },
            },
        };
        unsafe {
            GraphicsCommandList_ResourceBarrier(list.0, 1, &barrier);
        }
    }

    pub(super) fn submit(&self, uploads: Vec<Upload>, passes: Vec<Pass>) -> Result<()> {
        unsafe {
            let mut frames = self.frames.borrow_mut();
            let frame = &mut frames[self.slot.get()];
            self.wait(frame)?;
            check(CommandAllocator_Reset(frame.allocator.0))?;
            check(GraphicsCommandList_Reset(
                frame.list.0,
                frame.allocator.0,
                ptr::null_mut(),
            ))?;
            let list = &frame.list;
            let mut touched = HashMap::new();
            let mut staging_data = Vec::new();
            let mut offsets = Vec::new();
            for upload in &uploads {
                let offset = staging_data.len().div_ceil(512) * 512;
                staging_data.resize(offset, 0);
                offsets.push(offset as u64);
                match upload {
                    Upload::Buffer { data, .. } => staging_data.extend_from_slice(data),
                    Upload::Texture { size, data, .. } => {
                        let row = size.width as usize * 4;
                        let pitch = row.div_ceil(256) * 256;
                        for layer in 0..size.depth_or_array_layers as usize {
                            let start = staging_data.len().div_ceil(512) * 512;
                            staging_data.resize(start + pitch * size.height as usize, 0);
                            for y in 0..size.height as usize {
                                let src = (layer * size.height as usize + y) * row;
                                staging_data[start + y * pitch..start + y * pitch + row]
                                    .copy_from_slice(&data[src..src + row]);
                            }
                        }
                    }
                }
            }
            if !staging_data.is_empty() {
                let staging = match frame.staging.pop() {
                    Some(buffer) if frame.staging_capacity >= staging_data.len() as u64 => buffer,
                    _ => {
                        frame.staging_capacity = staging_data.len() as u64;
                        Self::resource(
                            &self.device,
                            &Self::buffer_desc(frame.staging_capacity),
                            true,
                        )?
                    }
                };
                let mut mapped = ptr::null_mut();
                check(Resource_Map(
                    staging.0,
                    0,
                    &D3D12_RANGE::default(),
                    &mut mapped,
                ))?;
                ptr::copy_nonoverlapping(staging_data.as_ptr(), mapped.cast(), staging_data.len());
                Resource_Unmap(staging.0, 0, ptr::null());
                for (upload, &offset) in uploads.iter().zip(&offsets) {
                    match upload {
                        Upload::Buffer {
                            buffer,
                            offset: destination,
                            data,
                        } => {
                            let native = &buffer.inner.native;
                            Self::transition(list, &native.object, &native.state, 1024);
                            GraphicsCommandList_CopyBufferRegion(
                                list.0,
                                native.object.0,
                                *destination,
                                staging.0,
                                offset,
                                data.len() as u64,
                            );
                            touched.insert(Rc::as_ptr(&buffer.inner), buffer.clone());
                        }
                        Upload::Texture {
                            texture,
                            mip,
                            origin,
                            size,
                            ..
                        } => {
                            let native = &texture.inner.native;
                            Self::transition(list, &native.object, &native.state, 1024);
                            let pitch = (size.width * 4).div_ceil(256) * 256;
                            let image =
                                (u64::from(pitch) * u64::from(size.height)).div_ceil(512) * 512;
                            for layer in 0..size.depth_or_array_layers {
                                let source = D3D12_TEXTURE_COPY_LOCATION {
                                    pResource: staging.0,
                                    Type: 1,
                                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                                        PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                                            Offset: offset + u64::from(layer) * image,
                                            Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                                                Format: format(native.format),
                                                Width: size.width,
                                                Height: size.height,
                                                Depth: 1,
                                                RowPitch: pitch,
                                            },
                                        },
                                    },
                                };
                                let destination = D3D12_TEXTURE_COPY_LOCATION {
                                    pResource: native.object.0,
                                    Type: 0,
                                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                                        SubresourceIndex: *mip + (origin.z + layer) * native.mips,
                                    },
                                };
                                GraphicsCommandList_CopyTextureRegion(
                                    list.0,
                                    &destination,
                                    origin.x,
                                    origin.y,
                                    0,
                                    &source,
                                    ptr::null(),
                                );
                            }
                        }
                    }
                }
                frame.staging.push(staging);
            }
            for buffer in touched.values() {
                let native = &buffer.inner.native;
                Self::transition(list, &native.object, &native.state, 193);
            }
            for upload in &uploads {
                if let Upload::Texture { texture, .. } = upload {
                    let native = &texture.inner.native;
                    Self::transition(list, &native.object, &native.state, 128);
                }
            }
            let heaps = [frame.resources.object.0, frame.samplers.object.0];
            GraphicsCommandList_SetDescriptorHeaps(list.0, 2, heaps.as_ptr());
            self.initialize_descriptors(frame, 0);
            let mut groups = HashMap::new();
            require(passes.len() <= 16, "Too many render passes");
            for (pass_index, pass) in passes.iter().enumerate() {
                let color = &pass.color.texture.inner.native;
                Self::transition(list, &color.object, &color.state, 4);
                let rtv = frame.colors.cpu(pass_index as u32);
                let dsv = frame.depths.cpu(pass_index as u32);
                let color_desc = D3D12_RENDER_TARGET_VIEW_DESC {
                    Format: format(color.format),
                    ViewDimension: 4,
                    ..Default::default()
                };
                Device_CreateRenderTargetView(self.device.0, color.object.0, &color_desc, rtv);
                let dsv_ptr = if let Some(depth) = &pass.depth {
                    let depth = &depth.texture.inner.native;
                    Self::transition(list, &depth.object, &depth.state, 16);
                    Device_CreateDepthStencilView(self.device.0, depth.object.0, ptr::null(), dsv);
                    &dsv
                } else {
                    ptr::null()
                };
                GraphicsCommandList_OMSetRenderTargets(list.0, 1, &rtv, 0, dsv_ptr);
                if let Some(clear) = pass.clear {
                    let clear = [
                        clear.r as f32,
                        clear.g as f32,
                        clear.b as f32,
                        clear.a as f32,
                    ];
                    GraphicsCommandList_ClearRenderTargetView(
                        list.0,
                        rtv,
                        clear.as_ptr(),
                        0,
                        ptr::null(),
                    );
                }
                if let Some(clear) = pass.clear_depth {
                    GraphicsCommandList_ClearDepthStencilView(
                        list.0,
                        dsv,
                        1,
                        clear,
                        0,
                        0,
                        ptr::null(),
                    );
                }
                GraphicsCommandList_IASetPrimitiveTopology(list.0, 4);
                for draw in &pass.draws {
                    let pipeline = &draw.pipeline.native;
                    GraphicsCommandList_SetPipelineState(list.0, pipeline.object.0);
                    GraphicsCommandList_SetGraphicsRootSignature(list.0, pipeline.root.0);
                    // Tier 1 requires initialized entries even when a shader does not use them.
                    GraphicsCommandList_SetGraphicsRootDescriptorTable(
                        list.0,
                        1,
                        frame.resources.gpu(0),
                    );
                    GraphicsCommandList_SetGraphicsRootDescriptorTable(
                        list.0,
                        3,
                        frame.samplers.gpu(0),
                    );
                    GraphicsCommandList_SetGraphicsRootShaderResourceView(
                        list.0,
                        4,
                        Resource_GetGPUVirtualAddress(self.sampler_indices.0),
                    );
                    if let Some(group) = &draw.group {
                        // A texture may have been rendered to by an earlier pass in this
                        // submission. Descriptor reuse does not imply that its resource
                        // state is still shader-readable, so transition on every bind.
                        for (_, resource) in group.entries.iter() {
                            if let Resource::Texture(view) = resource {
                                let texture = &view.texture.inner.native;
                                Self::transition(list, &texture.object, &texture.state, 128);
                            }
                        }
                        let key = Rc::as_ptr(&group.entries);
                        let descriptor_index = if let Some(&index) = groups.get(&key) {
                            index
                        } else {
                            let index = groups.len() as u32 + 1;
                            require(index <= 256, "Too many bind groups in a frame");
                            self.initialize_descriptors(frame, index);
                            for (binding, resource) in group.entries.iter() {
                                match resource {
                                    Resource::Texture(view) => {
                                        require(*binding == 1, "D3D12 texture binding must be 1");
                                        let texture = &view.texture.inner.native;
                                        let (dimension, resource) = match view.dimension {
                                            TextureViewDimension::D2 => (
                                                4,
                                                D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                                                    Texture2D: D3D12_TEX2D_SRV {
                                                        MipLevels: texture.mips,
                                                        ..Default::default()
                                                    },
                                                },
                                            ),
                                            TextureViewDimension::D2Array => (
                                                5,
                                                D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                                                    Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                                                        MipLevels: texture.mips,
                                                        ArraySize: texture.layers,
                                                        ..Default::default()
                                                    },
                                                },
                                            ),
                                        };
                                        let desc = D3D12_SHADER_RESOURCE_VIEW_DESC {
                                            Format: format(texture.format),
                                            ViewDimension: dimension,
                                            Shader4ComponentMapping: 5768,
                                            Anonymous: resource,
                                        };
                                        Device_CreateShaderResourceView(
                                            self.device.0,
                                            texture.object.0,
                                            &desc,
                                            frame.resources.cpu(index),
                                        );
                                    }
                                    Resource::Sampler(sampler) => {
                                        require(*binding == 2, "D3D12 sampler binding must be 2");
                                        Device_CreateSampler(
                                            self.device.0,
                                            &sampler.native.desc,
                                            frame.samplers.cpu(index),
                                        );
                                    }
                                    _ => {}
                                }
                            }
                            groups.insert(key, index);
                            index
                        };
                        GraphicsCommandList_SetGraphicsRootDescriptorTable(
                            list.0,
                            1,
                            frame.resources.gpu(descriptor_index),
                        );
                        GraphicsCommandList_SetGraphicsRootDescriptorTable(
                            list.0,
                            3,
                            frame.samplers.gpu(descriptor_index),
                        );
                        GraphicsCommandList_SetGraphicsRootShaderResourceView(
                            list.0,
                            4,
                            Resource_GetGPUVirtualAddress(self.sampler_indices.0),
                        );
                        for (binding, resource) in group.entries.iter() {
                            if let Resource::Buffer(buffer) = resource {
                                let native = &buffer.inner.native;
                                Self::transition(list, &native.object, &native.state, 193);
                                touched.insert(Rc::as_ptr(&buffer.inner), buffer.clone());
                                let address = Resource_GetGPUVirtualAddress(native.object.0);
                                match binding {
                                    0 => GraphicsCommandList_SetGraphicsRootConstantBufferView(
                                        list.0, 0, address,
                                    ),
                                    3 => GraphicsCommandList_SetGraphicsRootShaderResourceView(
                                        list.0, 2, address,
                                    ),
                                    _ => panic!("Unsupported D3D12 buffer binding"),
                                }
                            }
                        }
                    }
                    if let Some((stride, _)) = draw.pipeline.vertex {
                        let (buffer, range) =
                            draw.vertex.as_ref().expect("vertex buffer validated");
                        let native = &buffer.inner.native;
                        Self::transition(list, &native.object, &native.state, 193);
                        touched.insert(Rc::as_ptr(&buffer.inner), buffer.clone());
                        let view = D3D12_VERTEX_BUFFER_VIEW {
                            BufferLocation: Resource_GetGPUVirtualAddress(native.object.0)
                                + range.start,
                            SizeInBytes: u32::try_from(range.end - range.start)
                                .expect("Vertex buffer too large"),
                            StrideInBytes: stride as u32,
                        };
                        GraphicsCommandList_IASetVertexBuffers(list.0, 0, 1, &view);
                    }
                    let v = draw.viewport;
                    let viewport = D3D12_VIEWPORT {
                        TopLeftX: v[0],
                        TopLeftY: v[1],
                        Width: v[2],
                        Height: v[3],
                        MinDepth: v[4],
                        MaxDepth: v[5],
                    };
                    GraphicsCommandList_RSSetViewports(list.0, 1, &viewport);
                    let s = draw.scissor;
                    let rect = RECT {
                        left: s[0] as i32,
                        top: s[1] as i32,
                        right: (s[0] + s[2]) as i32,
                        bottom: (s[1] + s[3]) as i32,
                    };
                    GraphicsCommandList_RSSetScissorRects(list.0, 1, &rect);
                    GraphicsCommandList_DrawInstanced(
                        list.0,
                        draw.vertices.len() as u32,
                        draw.instances.len() as u32,
                        draw.vertices.start,
                        draw.instances.start,
                    );
                }
                if color.index.is_some() {
                    Self::transition(list, &color.object, &color.state, 0);
                }
            }
            check(GraphicsCommandList_Close(list.0))?;
            CommandQueue_ExecuteCommandLists(self.queue.0, 1, &list.0);
            // Buffers decay to COMMON at the ExecuteCommandLists boundary.
            for buffer in touched.values() {
                buffer.inner.native.state.set(0);
            }
            frame.fence = self.signal()?;
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
            let pitch = width
                .checked_mul(4)
                .ok_or_else(|| Error("Bitmap row is too wide".into()))?
                .div_ceil(256)
                * 256;
            let length = u64::from(pitch) * u64::from(height);
            let heap = D3D12_HEAP_PROPERTIES {
                Type: 3, // D3D12_HEAP_TYPE_READBACK
                CreationNodeMask: 1,
                VisibleNodeMask: 1,
                ..Default::default()
            };
            let readback = Com::create(|out| {
                Device_CreateCommittedResource(
                    self.device.0,
                    &heap,
                    0,
                    &Self::buffer_desc(length),
                    1024,
                    ptr::null(),
                    &ID3D12Resource,
                    out,
                )
            })?;
            let mut frames = self.frames.borrow_mut();
            let frame = &mut frames[self.slot.get()];
            self.wait(frame)?;
            check(CommandAllocator_Reset(frame.allocator.0))?;
            check(GraphicsCommandList_Reset(
                frame.list.0,
                frame.allocator.0,
                ptr::null_mut(),
            ))?;
            Self::transition(&frame.list, &texture.object, &texture.state, 2048);
            let source = D3D12_TEXTURE_COPY_LOCATION {
                pResource: texture.object.0,
                Type: 0,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    SubresourceIndex: 0,
                },
            };
            let destination = D3D12_TEXTURE_COPY_LOCATION {
                pResource: readback.0,
                Type: 1,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                        Offset: 0,
                        Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                            Format: format(texture.format),
                            Width: width,
                            Height: height,
                            Depth: 1,
                            RowPitch: pitch,
                        },
                    },
                },
            };
            GraphicsCommandList_CopyTextureRegion(
                frame.list.0,
                &destination,
                0,
                0,
                0,
                &source,
                ptr::null(),
            );
            check(GraphicsCommandList_Close(frame.list.0))?;
            CommandQueue_ExecuteCommandLists(self.queue.0, 1, &frame.list.0);
            frame.fence = self.signal()?;
            self.wait_value(frame.fence)?;
            let mut mapped = ptr::null_mut();
            check(Resource_Map(
                readback.0,
                0,
                &D3D12_RANGE {
                    Begin: 0,
                    End: length as usize,
                },
                &mut mapped,
            ))?;
            let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
            for row in 0..height as usize {
                let start = mapped.cast::<u8>().add(row * pitch as usize);
                bytes.extend_from_slice(std::slice::from_raw_parts(start, width as usize * 4));
            }
            Resource_Unmap(readback.0, 0, ptr::null());
            Ok(bytes)
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if let Ok(value) = self.signal() {
            let _ = self.wait_value(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_only_handles_unavailable_devices() {
        assert!(InitError::device(Error(String::new()), 0x887a0004_u32 as i32).fallback);
        assert!(!InitError::device(Error(String::new()), 0x8007000e_u32 as i32).fallback);
        assert!(!InitError::device(Error(String::new()), 0x80070057_u32 as i32).fallback);
    }

    #[test]
    fn warp_initializes_the_rendering_resources() {
        let context = Context::with_driver(true).unwrap_or_else(|error| panic!("{}", error.error));
        let frames = context.frames.borrow();
        assert_eq!(frames.len(), 2);
        context.initialize_descriptors(&frames[0], 0);
        context.initialize_descriptors(&frames[0], 256);
    }
}
