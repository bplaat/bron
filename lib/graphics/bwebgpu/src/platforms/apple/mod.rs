/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Metal renderer; objc2 supplies messaging and ownership only.

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void};
use std::ptr;
use std::rc::Rc;

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
#[cfg(feature = "window")]
pub(crate) use surface::{Factory, Surface};

use super::*;
use crate::platform::headers::*;

mod headers;
#[cfg(feature = "window")]
#[cfg_attr(target_os = "ios", path = "ios.rs")]
mod surface;

// SAFETY: Callers choose owned() only for +1 Objective-C results and retained()
// only for live +0 results. Retained releases the corresponding reference.
unsafe fn owned(value: *mut AnyObject) -> Result<Object> {
    unsafe { Retained::from_raw(value) }.ok_or_else(|| Error("Metal returned nil".into()))
}
unsafe fn retained(value: *mut AnyObject) -> Result<Object> {
    unsafe { Retained::retain(value) }.ok_or_else(|| Error("Metal returned nil".into()))
}

fn string(value: &str) -> Object {
    unsafe {
        let object: *mut AnyObject = msg_send![class!(NSString), alloc];
        owned(msg_send![object, initWithBytes: value.as_ptr().cast::<c_void>(), length: value.len(), encoding: 4usize])
            .expect("NSString allocation")
    }
}

fn error(value: *mut AnyObject) -> Error {
    if value.is_null() {
        return Error("Metal operation failed".into());
    }
    unsafe {
        let description: *mut AnyObject = msg_send![value, localizedDescription];
        let text: *const c_char = msg_send![description, UTF8String];
        Error(
            std::ffi::CStr::from_ptr(text)
                .to_string_lossy()
                .into_owned(),
        )
    }
}
const fn format(value: TextureFormat) -> usize {
    match value {
        TextureFormat::Rgba8Unorm => 70,
        TextureFormat::Rgba8UnormSrgb => 71,
        TextureFormat::Bgra8Unorm => 80,
        TextureFormat::Bgra8UnormSrgb => 81,
        TextureFormat::Depth32Float => 252,
    }
}

pub(super) struct Context {
    device: Object,
    queue: Object,
    frames: RefCell<[Option<Frame>; 2]>,
    slot: Cell<usize>,
    // One shared-memory upload buffer per frame slot, grown on demand and reused.
    staging: RefCell<[Option<Object>; 2]>,
    pub(crate) pool: RefCell<Pool>,
}

struct Frame {
    command: Object,
    presentation: Option<Object>,
    uploads: Vec<Upload>,
    passes: Vec<Pass>,
}

pub(super) struct Buffer {
    object: Object,
}

pub(super) struct Texture {
    object: Object,
    #[cfg(feature = "window")]
    drawable: Option<Object>,
}

pub(super) struct Sampler {
    object: Object,
}

pub(super) struct Pipeline {
    object: Object,
    depth: Option<Object>,
    cull: bool,
}

impl Context {
    pub(super) fn new() -> Result<Rc<Self>> {
        unsafe {
            let device = owned(MTLCreateSystemDefaultDevice())?;
            let queue = owned(msg_send![&*device, newCommandQueue])?;
            Ok(Rc::new(Self {
                device,
                queue,
                frames: RefCell::new([None, None]),
                slot: Cell::new(0),
                staging: RefCell::new([None, None]),
                pool: RefCell::default(),
            }))
        }
    }

    fn wait(&self, slot: usize) -> Result<()> {
        if let Some(frame) = self.frames.borrow_mut()[slot].take() {
            unsafe {
                for command in std::iter::once(&frame.command).chain(frame.presentation.as_ref()) {
                    let _: () = msg_send![&**command, waitUntilCompleted];
                    let status: usize = msg_send![&**command, status];
                    if status == 5 {
                        return Err(error(msg_send![&**command, error]));
                    }
                }
            }
            self.pool.borrow_mut().retire(frame.uploads, frame.passes);
        }
        Ok(())
    }

    /// Copy the upload arena into the current slot's staging buffer, which the GPU finished with.
    fn stage(&self, data: &[u8]) -> Result<Object> {
        unsafe {
            let mut staging = self.staging.borrow_mut();
            let slot = &mut staging[self.slot.get()];
            let length: usize = slot
                .as_ref()
                .map_or(0, |buffer| msg_send![&**buffer, length]);
            if length < data.len() {
                *slot = Some(owned(
                    msg_send![&*self.device, newBufferWithLength: data.len().next_power_of_two(), options: 0usize],
                )?);
            }
            let source = slot.clone().expect("staging buffer");
            let contents: *mut c_void = msg_send![&*source, contents];
            ptr::copy_nonoverlapping(data.as_ptr(), contents.cast(), data.len());
            Ok(source)
        }
    }

    pub(super) fn buffer(&self, size: u64, _: BufferUsages) -> Result<Buffer> {
        unsafe {
            Ok(Buffer {
                object: owned(
                    msg_send![&*self.device, newBufferWithLength: size as usize, options: 32usize],
                )?,
            })
        }
    }

    pub(super) fn texture(&self, desc: &TextureDescriptor<'_>) -> Result<Texture> {
        autoreleasepool(|_| unsafe {
            let descriptor = owned(msg_send![class!(MTLTextureDescriptor), new])?;
            let _: () = msg_send![&*descriptor, setTextureType: if desc.size.depth_or_array_layers > 1 { 3usize } else { 2usize }];
            let _: () = msg_send![&*descriptor, setPixelFormat: format(desc.format)];
            let _: () = msg_send![&*descriptor, setWidth: desc.size.width as usize];
            let _: () = msg_send![&*descriptor, setHeight: desc.size.height as usize];
            let _: () =
                msg_send![&*descriptor, setArrayLength: desc.size.depth_or_array_layers as usize];
            let _: () = msg_send![&*descriptor, setMipmapLevelCount: desc.mip_level_count as usize];
            let _: () = msg_send![&*descriptor, setStorageMode: 2usize];
            let usage = usize::from(desc.usage.contains(TextureUsages::TEXTURE_BINDING))
                | if desc.usage.contains(TextureUsages::RENDER_ATTACHMENT) {
                    4
                } else {
                    0
                };
            let _: () = msg_send![&*descriptor, setUsage: usage];
            Ok(Texture {
                object: owned(msg_send![&*self.device, newTextureWithDescriptor: &*descriptor])?,
                #[cfg(feature = "window")]
                drawable: None,
            })
        })
    }

    pub(super) fn sampler(&self, desc: &SamplerDescriptor<'_>) -> Result<Sampler> {
        autoreleasepool(|_| unsafe {
            let descriptor = owned(msg_send![class!(MTLSamplerDescriptor), new])?;
            let _: () = msg_send![&*descriptor, setSAddressMode: if desc.address_mode_u == AddressMode::Repeat { 2usize } else { 0usize }];
            let _: () = msg_send![&*descriptor, setTAddressMode: if desc.address_mode_v == AddressMode::Repeat { 2usize } else { 0usize }];
            let _: () = msg_send![&*descriptor, setMagFilter: usize::from(desc.mag_filter == FilterMode::Linear)];
            let _: () = msg_send![&*descriptor, setMinFilter: usize::from(desc.min_filter == FilterMode::Linear)];
            let _: () = msg_send![&*descriptor, setMipFilter: if desc.mipmap_filter == FilterMode::Linear { 2usize } else { 1usize }];
            Ok(Sampler {
                object: owned(
                    msg_send![&*self.device, newSamplerStateWithDescriptor: &*descriptor],
                )?,
            })
        })
    }

    fn function(&self, shader: &ShaderModule, entry: Option<&str>) -> Result<Object> {
        unsafe {
            let mut err = ptr::null_mut::<AnyObject>();
            let error_out = (&mut err as *mut *mut AnyObject).cast::<c_void>();
            let library: *mut AnyObject = match &shader.source {
                NativeSource::Binary(_) => {
                    let bytes = shader
                        .binary(entry)
                        .ok_or_else(|| Error("Metal entry point has no library".into()))?;
                    let data = owned(dispatch_data_create(
                        bytes.as_ptr().cast(),
                        bytes.len(),
                        ptr::null_mut(),
                        ptr::null_mut(),
                    ))?;
                    msg_send![&*self.device, newLibraryWithData: &*data, error: error_out]
                }
                NativeSource::Text(source) => {
                    msg_send![&*self.device, newLibraryWithSource: &*string(source), options: ptr::null::<AnyObject>(), error: error_out]
                }
                NativeSource::SpirV(_) => {
                    return Err(Error("Metal requires a Metal library".into()));
                }
            };
            if library.is_null() {
                return Err(error(err));
            }
            let library = owned(library)?;
            owned(msg_send![&*library, newFunctionWithName: &*string(shader.entry(entry))])
        }
    }

    pub(super) fn pipeline(&self, desc: &RenderPipelineDescriptor<'_>) -> Result<Pipeline> {
        autoreleasepool(|_| unsafe {
            let fragment = desc.fragment.as_ref().expect("fragment state");
            let color = fragment.targets[0].expect("color target");
            let vertex = self.function(desc.vertex.module, desc.vertex.entry_point)?;
            let fragment = self.function(fragment.module, fragment.entry_point)?;
            let descriptor = owned(msg_send![class!(MTLRenderPipelineDescriptor), new])?;
            let _: () = msg_send![&*descriptor, setVertexFunction: &*vertex];
            let _: () = msg_send![&*descriptor, setFragmentFunction: &*fragment];
            let attachments: *mut AnyObject = msg_send![&*descriptor, colorAttachments];
            let attachment: *mut AnyObject =
                msg_send![attachments, objectAtIndexedSubscript: 0usize];
            let _: () = msg_send![attachment, setPixelFormat: format(color.format)];
            if let Some(blend) = color.blend {
                // MTLBlendFactorOne for premultiplied sources, otherwise MTLBlendFactorSourceAlpha.
                let source = if blend.premultiplied { 1usize } else { 4usize };
                let _: () = msg_send![attachment, setBlendingEnabled: Bool::YES];
                let _: () = msg_send![attachment, setSourceRGBBlendFactor: source];
                let _: () = msg_send![attachment, setDestinationRGBBlendFactor: 5usize];
                let _: () = msg_send![attachment, setSourceAlphaBlendFactor: 1usize];
                let _: () = msg_send![attachment, setDestinationAlphaBlendFactor: 5usize];
            }
            if desc.depth_stencil.is_some() {
                let _: () = msg_send![&*descriptor, setDepthAttachmentPixelFormat: 252usize];
            }
            if let Some(Some(layout)) = desc.vertex.buffers.first() {
                let vertex_descriptor = owned(msg_send![class!(MTLVertexDescriptor), new])?;
                let attributes: *mut AnyObject = msg_send![&*vertex_descriptor, attributes];
                for attribute in layout.attributes {
                    let native: *mut AnyObject = msg_send![
                        attributes,
                        objectAtIndexedSubscript: attribute.shader_location as usize
                    ];
                    let format = match attribute.format {
                        VertexFormat::Float32x2 => 29usize,
                        VertexFormat::Float32x3 => 30usize,
                        VertexFormat::Uint32x2 => 37usize,
                    };
                    let _: () = msg_send![native, setFormat: format];
                    let _: () = msg_send![native, setOffset: attribute.offset as usize];
                    let _: () = msg_send![native, setBufferIndex: 16usize];
                }
                let layouts: *mut AnyObject = msg_send![&*vertex_descriptor, layouts];
                let native_layout: *mut AnyObject =
                    msg_send![layouts, objectAtIndexedSubscript: 16usize];
                let _: () = msg_send![native_layout, setStride: layout.array_stride as usize];
                let _: () = msg_send![native_layout, setStepFunction: if layout.step_mode == VertexStepMode::Vertex { 1usize } else { 2usize }];
                let _: () = msg_send![native_layout, setStepRate: 1usize];
                let _: () = msg_send![&*descriptor, setVertexDescriptor: &*vertex_descriptor];
            }
            let mut err = ptr::null_mut::<AnyObject>();
            let error_out = (&mut err as *mut *mut AnyObject).cast::<c_void>();
            let pipeline: *mut AnyObject = msg_send![&*self.device, newRenderPipelineStateWithDescriptor: &*descriptor, error: error_out];
            if pipeline.is_null() {
                return Err(error(err));
            }
            let object = owned(pipeline)?;
            let depth = desc
                .depth_stencil
                .map(|depth| -> Result<Object> {
                    let descriptor = owned(msg_send![class!(MTLDepthStencilDescriptor), new])?;
                    let _: () = msg_send![&*descriptor, setDepthCompareFunction: if depth.depth_compare == Some(CompareFunction::Always) { 7usize } else { 3usize }];
                    let write_enabled = if depth.depth_write_enabled.unwrap_or(false) {
                        Bool::YES
                    } else {
                        Bool::NO
                    };
                    let _: () = msg_send![&*descriptor, setDepthWriteEnabled: write_enabled];
                    owned(msg_send![&*self.device, newDepthStencilStateWithDescriptor: &*descriptor])
                })
                .transpose()?;
            Ok(Pipeline {
                object,
                depth,
                cull: desc.primitive.cull_mode.is_some(),
            })
        })
    }

    pub(super) fn submit(
        &self,
        uploads: Vec<Upload>,
        data: &[u8],
        passes: Vec<Pass>,
    ) -> Result<()> {
        self.wait(self.slot.get())?;
        autoreleasepool(|_| unsafe {
            let command = retained(msg_send![&*self.queue, commandBuffer])?;
            if !uploads.is_empty() {
                let source = self.stage(data)?;
                let blit = retained(msg_send![&*command, blitCommandEncoder])?;
                for upload in &uploads {
                    match upload {
                        Upload::Buffer {
                            buffer,
                            offset,
                            data,
                        } => {
                            let _: () = msg_send![&*blit, copyFromBuffer: &*source, sourceOffset: data.start, toBuffer: &*buffer.inner.native.object, destinationOffset: *offset as usize, size: data.len()];
                        }
                        Upload::Texture {
                            texture,
                            mip,
                            origin,
                            size,
                            data,
                        } => {
                            let row = size.width as usize * 4;
                            let image = row * size.height as usize;
                            for layer in 0..size.depth_or_array_layers {
                                let _: () = msg_send![&*blit, copyFromBuffer: &*source, sourceOffset: data.start + layer as usize * image, sourceBytesPerRow: row, sourceBytesPerImage: image, sourceSize: MSize { width: size.width as usize, height: size.height as usize, depth: 1 }, toTexture: &*texture.inner.native.object, destinationSlice: (origin.z + layer) as usize, destinationLevel: *mip as usize, destinationOrigin: MOrigin { x: origin.x as usize, y: origin.y as usize, z: 0 }];
                            }
                        }
                    }
                }
                let _: () = msg_send![&*blit, endEncoding];
            }
            for pass in &passes {
                let descriptor = owned(msg_send![class!(MTLRenderPassDescriptor), new])?;
                let colors: *mut AnyObject = msg_send![&*descriptor, colorAttachments];
                let color: *mut AnyObject = msg_send![colors, objectAtIndexedSubscript: 0usize];
                let _: () = msg_send![color, setTexture: &*pass.color.texture.inner.native.object];
                let _: () = msg_send![color, setLoadAction: if pass.clear.is_some() { 2usize } else { 1usize }];
                let _: () = msg_send![color, setStoreAction: 1usize];
                if let Some(clear) = pass.clear {
                    let _: () = msg_send![color, setClearColor: Clear { r: clear.r, g: clear.g, b: clear.b, a: clear.a }];
                }
                if let Some(depth_view) = &pass.depth {
                    let depth: *mut AnyObject = msg_send![&*descriptor, depthAttachment];
                    let _: () =
                        msg_send![depth, setTexture: &*depth_view.texture.inner.native.object];
                    let _: () = msg_send![depth, setLoadAction: if pass.clear_depth.is_some() { 2usize } else { 1usize }];
                    let _: () = msg_send![depth, setStoreAction: usize::from(pass.store_depth)];
                    if let Some(clear) = pass.clear_depth {
                        let _: () = msg_send![depth, setClearDepth: clear as f64];
                    }
                }
                let render = retained(
                    msg_send![&*command, renderCommandEncoderWithDescriptor: &*descriptor],
                )?;
                let _: () = msg_send![&*render, setFrontFacingWinding: 1usize];
                for draw in &pass.draws {
                    let native = &draw.pipeline.native;
                    let _: () = msg_send![&*render, setRenderPipelineState: &*native.object];
                    if let Some(depth) = &native.depth {
                        let _: () = msg_send![&*render, setDepthStencilState: &**depth];
                    }
                    let _: () =
                        msg_send![&*render, setCullMode: if native.cull { 2usize } else { 0usize }];
                    for (binding, resource) in
                        draw.group.iter().flat_map(|group| group.entries.iter())
                    {
                        match resource {
                            Resource::Buffer(buffer) => {
                                let _: () = msg_send![&*render, setVertexBuffer: &*buffer.inner.native.object, offset: 0usize, atIndex: *binding as usize];
                                let _: () = msg_send![&*render, setFragmentBuffer: &*buffer.inner.native.object, offset: 0usize, atIndex: *binding as usize];
                                if buffer.inner.usage.contains(BufferUsages::STORAGE) {
                                    let length = u32::try_from(buffer.inner.size)
                                        .expect("Metal storage buffer too large");
                                    let _: () = msg_send![&*render, setVertexBytes: (&length as *const u32).cast::<c_void>(), length: 4usize, atIndex: 30usize];
                                }
                            }
                            Resource::Texture(view) => {
                                let _: () = msg_send![&*render, setFragmentTexture: &*view.texture.inner.native.object, atIndex: *binding as usize];
                            }
                            Resource::Sampler(sampler) => {
                                let _: () = msg_send![&*render, setFragmentSamplerState: &*sampler.native.object, atIndex: *binding as usize];
                            }
                        }
                    }
                    if let Some((buffer, range)) = &draw.vertex {
                        let _: () = msg_send![&*render, setVertexBuffer: &*buffer.inner.native.object, offset: range.start as usize, atIndex: 16usize];
                    }
                    let v = draw.viewport;
                    let _: () = msg_send![&*render, setViewport: Viewport { x: v[0] as f64, y: v[1] as f64, width: v[2] as f64, height: v[3] as f64, near: v[4] as f64, far: v[5] as f64 }];
                    let s = draw.scissor;
                    let _: () = msg_send![&*render, setScissorRect: Scissor { x: s[0] as usize, y: s[1] as usize, width: s[2] as usize, height: s[3] as usize }];
                    let _: () = msg_send![&*render, drawPrimitives: 3usize, vertexStart: draw.vertices.start as usize, vertexCount: draw.vertices.len(), instanceCount: draw.instances.len(), baseInstance: draw.instances.start as usize];
                }
                let _: () = msg_send![&*render, endEncoding];
            }
            let _: () = msg_send![&*command, commit];
            self.frames.borrow_mut()[self.slot.get()] = Some(Frame {
                command,
                presentation: None,
                uploads,
                passes,
            });
            Ok(())
        })
    }

    #[cfg(feature = "image")]
    pub(super) fn read_texture(
        &self,
        texture: &Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        self.wait(self.slot.get())?;
        autoreleasepool(|_| unsafe {
            let length = usize::try_from(u64::from(width) * u64::from(height) * 4)
                .map_err(|_| Error("Bitmap is too large".into()))?;
            let buffer =
                owned(msg_send![&*self.device, newBufferWithLength: length, options: 0usize])?;
            let command = retained(msg_send![&*self.queue, commandBuffer])?;
            let blit = retained(msg_send![&*command, blitCommandEncoder])?;
            let _: () = msg_send![&*blit, copyFromTexture: &*texture.object, sourceSlice: 0usize, sourceLevel: 0usize, sourceOrigin: MOrigin { x: 0, y: 0, z: 0 }, sourceSize: MSize { width: width as usize, height: height as usize, depth: 1 }, toBuffer: &*buffer, destinationOffset: 0usize, destinationBytesPerRow: width as usize * 4, destinationBytesPerImage: length];
            let _: () = msg_send![&*blit, endEncoding];
            let _: () = msg_send![&*command, commit];
            let _: () = msg_send![&*command, waitUntilCompleted];
            let status: usize = msg_send![&*command, status];
            if status == 5 {
                return Err(error(msg_send![&*command, error]));
            }
            let data: *const c_void = msg_send![&*buffer, contents];
            Ok(std::slice::from_raw_parts(data.cast::<u8>(), length).to_vec())
        })
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        let _ = self.wait(0);
        let _ = self.wait(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu() -> (Device, Queue) {
        let shared = Rc::new(Shared {
            native: Context::new().expect("Metal device required"),
            uploads: RefCell::default(),
        });
        (
            Device {
                shared: shared.clone(),
            },
            Queue { shared },
        )
    }

    fn pixels(device: &Device, texture: &super::super::Texture) -> Vec<u8> {
        autoreleasepool(|_| unsafe {
            let context = &device.shared.native;
            let size = texture.inner.size;
            let pitch = (size.width as usize * 4).div_ceil(256) * 256;
            let length = pitch * size.height as usize;
            let buffer =
                owned(msg_send![&*context.device, newBufferWithLength: length, options: 0usize])
                    .unwrap();
            let command = retained(msg_send![&*context.queue, commandBuffer]).unwrap();
            let blit = retained(msg_send![&*command, blitCommandEncoder]).unwrap();
            let _: () = msg_send![&*blit, copyFromTexture: &*texture.inner.native.object, sourceSlice: 0usize, sourceLevel: 0usize, sourceOrigin: MOrigin { x: 0, y: 0, z: 0 }, sourceSize: MSize { width: size.width as usize, height: size.height as usize, depth: 1 }, toBuffer: &*buffer, destinationOffset: 0usize, destinationBytesPerRow: pitch, destinationBytesPerImage: length];
            let _: () = msg_send![&*blit, endEncoding];
            let _: () = msg_send![&*command, commit];
            let _: () = msg_send![&*command, waitUntilCompleted];
            let status: usize = msg_send![&*command, status];
            assert_eq!(status, 4, "Readback command failed");
            let contents: *const c_void = msg_send![&*buffer, contents];
            let bytes = std::slice::from_raw_parts(contents.cast::<u8>(), length);
            bytes
                .chunks_exact(pitch)
                .flat_map(|row| &row[..size.width as usize * 4])
                .copied()
                .collect()
        })
    }

    #[test]
    fn native_render_without_bindings_or_depth_blends() {
        let (device, queue) = gpu();
        let color = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: None,
            entries: &[("vs", "vs"), ("fs", "fs")],
            source: ShaderSource::Text(
                r#"
                #include <metal_stdlib>
                using namespace metal;
                vertex float4 vs(uint index [[vertex_id]]) {
                    const float2 points[] = { float2(-1,-1), float2(3,-1), float2(-1,3) };
                    return float4(points[index], 0, 1);
                }
                fragment float4 fs() { return float4(0, 0.5, 0, 0.5); }
                "#,
            ),
        });
        // A half-transparent green source over opaque red, as straight and premultiplied alpha.
        for (blend, expected) in [
            (None, [0, 128, 0, 128]),
            (Some(BlendState::ALPHA_BLENDING), [128, 64, 0, 255]),
            (
                Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                [128, 128, 0, 255],
            ),
        ] {
            let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(ColorTargetState {
                        format: TextureFormat::Rgba8Unorm,
                        blend,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            let view = color.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(Color {
                                r: 1.,
                                g: 0.,
                                b: 0.,
                                a: 1.,
                            }),
                            store: StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                });
                pass.set_pipeline(&pipeline);
                pass.draw(0..3, 0..1);
            }
            queue.submit([encoder.finish()]);
            for pixel in pixels(&device, &color).as_chunks::<4>().0 {
                for (actual, expected) in pixel.iter().zip(expected) {
                    assert!(actual.abs_diff(expected) <= 1, "{pixel:?} != {expected:?}");
                }
            }
        }
    }

    #[test]
    fn invalid_upload_alignment_and_attachment_feedback_are_rejected() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let (device, _) = gpu();
        assert!(
            catch_unwind(AssertUnwindSafe(|| device.create_buffer(
                &BufferDescriptor {
                    label: None,
                    size: 3,
                    usage: BufferUsages::VERTEX,
                    mapped_at_creation: true,
                }
            )))
            .is_err()
        );
        // Odd sizes remain valid for unmapped buffers, which can receive aligned writes.
        let _buffer = device.create_buffer(&BufferDescriptor {
            label: None,
            size: 7,
            usage: BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let descriptor = TextureDescriptor {
            label: None,
            size: Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        let color = device.create_texture(&descriptor);
        let other = device.create_texture(&descriptor);
        let depth = device.create_texture(&TextureDescriptor {
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT,
            ..descriptor
        });
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
        });
        for (texture, reject) in [(&other, false), (&color, true)] {
            // A separate view of the same texture must still be detected as feedback.
            let view = texture.create_view(&Default::default());
            let group = device.create_bind_group(&BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&view),
                }],
            });
            assert_eq!(
                catch_unwind(AssertUnwindSafe(|| pass.set_bind_group(0, &group, &[]))).is_err(),
                reject
            );
        }
    }

    #[test]
    fn native_render_uploads_depth_blending_and_lifetimes() {
        let (device, queue) = gpu();
        let color = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth = device.create_texture(&TextureDescriptor {
            label: None,
            size: color.inner.size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let tiles = device.create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width: 2,
                height: 2,
                depth_or_array_layers: 2,
            },
            mip_level_count: 2,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, data) in [[255, 0, 0, 255], [0, 255, 0, 255]].iter().enumerate() {
            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &tiles,
                    mip_level: 1,
                    origin: Origin3d {
                        z: layer as u32,
                        ..Default::default()
                    },
                    aspect: TextureAspect::All,
                },
                data,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
        }
        let uniforms = device.create_buffer(&BufferDescriptor {
            label: None,
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let origins = device.create_buffer(&BufferDescriptor {
            label: None,
            size: 32,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let values: Vec<u8> = [0., 0., 0.75f32, 0., 0., 0., 0.5, 0.]
            .iter()
            .flat_map(|n| n.to_ne_bytes())
            .collect();
        queue.write_buffer(&origins, 0, &values);
        let vertices = device.create_buffer(&BufferDescriptor {
            label: None,
            size: 24,
            usage: BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        {
            let mut mapped = vertices.slice(..).get_mapped_range_mut().unwrap();
            let bytes: Vec<u8> = [0u32, 0, 1, 1, 0, 0]
                .iter()
                .flat_map(|n| n.to_ne_bytes())
                .collect();
            mapped.slice(0..bytes.len()).copy_from_slice(&bytes);
            assert!(vertices.slice(..).get_mapped_range_mut().is_err());
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vertices.unmap()))
                    .is_err()
            );
        }
        vertices.unmap();
        assert!(vertices.slice(..).get_mapped_range_mut().is_err());
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
        });
        let shader = device.create_shader_module(ShaderModuleDescriptor { label: None, entries: &[("vs", "vs"), ("fs", "fs")], source: ShaderSource::Text(r#"
            #include <metal_stdlib>
            using namespace metal;
            struct In { uint2 packed [[attribute(0)]]; };
            struct Out { float4 position [[position]]; uint layer [[flat]]; };
            vertex Out vs(In in [[stage_in]], uint index [[vertex_id]], device const float4* origins [[buffer(3)]]) {
                const float2 points[] = { float2(-1,-1), float2(3,-1), float2(-1,3) };
                return { float4(points[index], origins[in.packed.y].z, 1), in.packed.x };
            }
            fragment float4 fs(Out in [[stage_in]], constant float4& tint [[buffer(0)]], texture2d_array<float> tiles [[texture(1)]], sampler s [[sampler(2)]]) {
                return tiles.sample(s, float2(0.5), in.layer, level(1)) * tint;
            }
        "#) });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(VertexBufferLayout {
                    array_stride: 8,
                    step_mode: VertexStepMode::Instance,
                    attributes: &vertex_attr_array![0 => Uint32x2],
                })],
            },
            primitive: PrimitiveState {
                cull_mode: Some(Face::Back),
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format: TextureFormat::Rgba8Unorm,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&Default::default());
        let view = tiles.create_view(&TextureViewDescriptor {
            dimension: Some(TextureViewDimension::D2Array),
        });
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: origins.as_entire_binding(),
                },
            ],
        });
        let vertex_lifetime = Rc::downgrade(&vertices.inner);
        for alpha in [0.5f32, 1., 0.25] {
            // Two writes to the same buffer must be observed in queue order.
            queue.write_buffer(&uniforms, 0, &[0; 16]);
            queue.write_buffer(
                &uniforms,
                0,
                &[1f32, 1., 1., alpha]
                    .iter()
                    .flat_map(|n| n.to_ne_bytes())
                    .collect::<Vec<_>>(),
            );
            let mut encoder = device.create_command_encoder(&Default::default());
            for load in [false, true] {
                let color_view = color.create_view(&Default::default());
                let depth_view = depth.create_view(&Default::default());
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &color_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: if load {
                                LoadOp::Load
                            } else {
                                LoadOp::Clear(Color {
                                    r: 1.,
                                    g: 0.,
                                    b: 0.,
                                    a: 1.,
                                })
                            },
                            store: StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(Operations {
                            load: if load {
                                LoadOp::Load
                            } else {
                                LoadOp::Clear(1.)
                            },
                            store: StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                });
                pass.set_bind_group(0, &group, &[]);
                pass.set_pipeline(&pipeline);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_viewport(0., 0., 4., 4., 0., 1.);
                pass.set_scissor_rect(0, 0, 2, 4);
                if load {
                    // Loading both attachments must preserve the green image and its depth.
                    // The farther red instance must fail against the first pass's depth.
                    pass.draw(0..3, 2..3);
                } else {
                    pass.draw(0..3, 1..2);
                }
            }
            queue.submit([encoder.finish()]);
            let image = pixels(&device, &color);
            for (i, pixel) in image.as_chunks::<4>().0.iter().enumerate() {
                let expected = if i % 4 < 2 {
                    [
                        ((1. - alpha) * 255.).round() as u8,
                        (alpha * 255.).round() as u8,
                        0,
                        255,
                    ]
                } else {
                    [255, 0, 0, 255]
                };
                for (actual, expected) in pixel.iter().zip(expected) {
                    assert!(
                        actual.abs_diff(expected) <= 1,
                        "pixel {i}: {pixel:?} expected {expected}"
                    );
                }
            }
        }
        drop(vertices);
        drop(group);
        drop(uniforms);
        drop(origins);
        drop(view);
        drop(tiles);
        drop(pipeline);
        assert!(
            vertex_lifetime.upgrade().is_some(),
            "Submission must retain vertex data"
        );
        device.shared.native.wait(0).unwrap();
        device.shared.native.wait(1).unwrap();
        assert!(
            vertex_lifetime.upgrade().is_none(),
            "Completed submissions must release vertex data"
        );
    }
}
