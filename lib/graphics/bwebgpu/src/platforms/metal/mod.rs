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

// Fill `layer` with `view`, without implicit animations.
#[cfg(feature = "window")]
unsafe fn layout_layer(view: &AnyObject, layer: &AnyObject) {
    unsafe {
        let bounds: Rect = msg_send![view, bounds];
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: Bool::YES];
        let _: () = msg_send![layer, setFrame: bounds];
        let _: () = msg_send![class!(CATransaction), commit];
    }
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
    staging_pool: RefCell<[Vec<Object>; 2]>,
}

struct Frame {
    command: Object,
    presentation: Option<Object>,
    _uploads: Vec<Upload>,
    _passes: Vec<Pass>,
    _staging: Vec<Object>,
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

const fn blend_factor(value: BlendFactor) -> usize {
    match value {
        BlendFactor::Zero => 0,
        BlendFactor::One => 1,
        BlendFactor::Src => 2,
        BlendFactor::OneMinusSrc => 3,
        BlendFactor::SrcAlpha => 4,
        BlendFactor::OneMinusSrcAlpha => 5,
        BlendFactor::Dst => 6,
        BlendFactor::OneMinusDst => 7,
        BlendFactor::DstAlpha => 8,
        BlendFactor::OneMinusDstAlpha => 9,
        BlendFactor::SrcAlphaSaturated => 10,
    }
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
                staging_pool: RefCell::new([Vec::new(), Vec::new()]),
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
                self.staging_pool.borrow_mut()[slot] = frame._staging;
            }
        }
        Ok(())
    }

    fn stage(&self, data: &[u8]) -> Result<Object> {
        unsafe {
            let source = match self.staging_pool.borrow_mut()[self.slot.get()].pop() {
                Some(buffer)
                    if {
                        let length: usize = msg_send![&*buffer, length];
                        length >= data.len()
                    } =>
                {
                    buffer
                }
                _ => owned(
                    msg_send![&*self.device, newBufferWithLength: data.len(), options: 0usize],
                )?,
            };
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
            let _: () = msg_send![&*descriptor, setRAddressMode: if desc.address_mode_w == AddressMode::Repeat { 2usize } else { 0usize }];
            let _: () = msg_send![&*descriptor, setLodMinClamp: desc.lod_min_clamp];
            let _: () = msg_send![&*descriptor, setLodMaxClamp: desc.lod_max_clamp];
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
                let _: () = msg_send![attachment, setBlendingEnabled: Bool::YES];
                let _: () = msg_send![attachment, setSourceRGBBlendFactor: blend_factor(blend.color.src_factor)];
                let _: () = msg_send![attachment, setDestinationRGBBlendFactor: blend_factor(blend.color.dst_factor)];
                let _: () =
                    msg_send![attachment, setRgbBlendOperation: blend.color.operation as usize];
                let _: () = msg_send![attachment, setSourceAlphaBlendFactor: blend_factor(blend.alpha.src_factor.for_alpha())];
                let _: () = msg_send![attachment, setDestinationAlphaBlendFactor: blend_factor(blend.alpha.dst_factor.for_alpha())];
                let _: () =
                    msg_send![attachment, setAlphaBlendOperation: blend.alpha.operation as usize];
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
                        VertexFormat::Float32x4 => 31usize,
                        VertexFormat::Uint8x4 => 3usize,
                        VertexFormat::Unorm8x4 => 9usize,
                        VertexFormat::Sint16x4 => 18usize,
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
                    let _: () = msg_send![&*descriptor, setDepthCompareFunction: depth.depth_compare.unwrap_or(CompareFunction::LessEqual) as usize - 1];
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

    pub(super) fn submit(&self, uploads: Vec<Upload>, passes: Vec<Pass>) -> Result<()> {
        self.wait(self.slot.get())?;
        autoreleasepool(|_| unsafe {
            let command = retained(msg_send![&*self.queue, commandBuffer])?;
            let mut staging = Vec::new();
            if !uploads.is_empty() {
                let blit = retained(msg_send![&*command, blitCommandEncoder])?;
                for upload in &uploads {
                    match upload {
                        Upload::Buffer {
                            buffer,
                            offset,
                            data,
                        } => {
                            let source = self.stage(data)?;
                            let _: () = msg_send![&*blit, copyFromBuffer: &*source, sourceOffset: 0usize, toBuffer: &*buffer.inner.native.object, destinationOffset: *offset as usize, size: data.len()];
                            staging.push(source);
                        }
                        Upload::Texture {
                            texture,
                            mip,
                            origin,
                            size,
                            data,
                        } => {
                            let row = size.width as usize * 4;
                            let pitch = (row + 255) & !255;
                            let image = pitch * size.height as usize;
                            let mut padded = vec![0u8; image * size.depth_or_array_layers as usize];
                            for (src, dst) in
                                data.chunks_exact(row).zip(padded.chunks_exact_mut(pitch))
                            {
                                dst[..row].copy_from_slice(src);
                            }
                            let source = self.stage(&padded)?;
                            for layer in 0..size.depth_or_array_layers {
                                let _: () = msg_send![&*blit, copyFromBuffer: &*source, sourceOffset: layer as usize * image, sourceBytesPerRow: pitch, sourceBytesPerImage: image, sourceSize: MSize { width: size.width as usize, height: size.height as usize, depth: 1 }, toTexture: &*texture.inner.native.object, destinationSlice: (origin.z + layer) as usize, destinationLevel: *mip as usize, destinationOrigin: MOrigin { x: origin.x as usize, y: origin.y as usize, z: 0 }];
                            }
                            staging.push(source);
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
                    match &draw.kind {
                        DrawKind::Direct(vertices) => {
                            let _: () = msg_send![&*render, drawPrimitives: 3usize, vertexStart: vertices.start as usize, vertexCount: vertices.len(), instanceCount: draw.instances.len(), baseInstance: draw.instances.start as usize];
                        }
                        DrawKind::Indexed {
                            indices,
                            base_vertex,
                            binding,
                        } => {
                            let offset = binding.range.start
                                + u64::from(indices.start) * binding.format.byte_size();
                            let _: () = msg_send![&*render, drawIndexedPrimitives: 3usize, indexCount: indices.len(), indexType: usize::from(binding.format == IndexFormat::Uint32), indexBuffer: &*binding.buffer.inner.native.object, indexBufferOffset: offset as usize, instanceCount: draw.instances.len(), baseVertex: *base_vertex as isize, baseInstance: draw.instances.start as usize];
                        }
                    }
                }
                let _: () = msg_send![&*render, endEncoding];
            }
            let _: () = msg_send![&*command, commit];
            self.staging_pool.borrow_mut()[self.slot.get()].clear();
            self.frames.borrow_mut()[self.slot.get()] = Some(Frame {
                command,
                presentation: None,
                _uploads: uploads,
                _passes: passes,
                _staging: staging,
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
            uploads: RefCell::new(Vec::new()),
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
    fn native_render_without_bindings_or_depth() {
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
                fragment float4 fs() { return float4(0, 1, 0, 1); }
                "#,
            ),
        });
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
                    blend: None,
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
                        load: LoadOp::Clear(Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&pipeline);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        assert!(
            pixels(&device, &color)
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 255, 0, 255])
        );
    }

    #[test]
    fn native_depth_comparisons_and_custom_blending() {
        let (device, queue) = gpu();
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
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        };
        let color = device.create_texture(&descriptor);
        let depth = device.create_texture(&TextureDescriptor {
            format: TextureFormat::Depth32Float,
            ..descriptor
        });
        let color_view = color.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: None,
            entries: &[("vs", "vs"), ("fs", "fs")],
            source: ShaderSource::Text(
                r#"
                #include <metal_stdlib>
                using namespace metal;
                vertex float4 vs(uint index [[vertex_id]]) {
                    const float2 points[] = { float2(-1,-1), float2(3,-1), float2(-1,3) };
                    return float4(points[index], 0.5, 1);
                }
                fragment float4 fs() { return float4(1, 0, 0, 0.5); }
            "#,
            ),
        });
        let pipeline = |compare: Option<CompareFunction>, blend| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: compare.map(|compare| DepthStencilState {
                    format: TextureFormat::Depth32Float,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(compare),
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
                        blend,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let render = |pipeline: &RenderPipeline, clear_depth: Option<f32>| {
            let mut encoder = device.create_command_encoder(&Default::default());
            {
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
                    depth_stencil_attachment: clear_depth.map(|value| {
                        RenderPassDepthStencilAttachment {
                            view: &depth_view,
                            depth_ops: Some(Operations {
                                load: LoadOp::Clear(value),
                                store: StoreOp::Discard,
                            }),
                            stencil_ops: None,
                        }
                    }),
                });
                pass.set_pipeline(pipeline);
                pass.draw(0..3, 0..1);
            }
            queue.submit([encoder.finish()]);
            pixels(&device, &color)
        };
        for operation in [BlendOperation::Min, BlendOperation::Max] {
            for invalid_alpha in [false, true] {
                let component = BlendComponent {
                    operation,
                    ..BlendComponent::REPLACE
                };
                let blend = if invalid_alpha {
                    BlendState {
                        alpha: component,
                        ..BlendState::REPLACE
                    }
                } else {
                    BlendState {
                        color: component,
                        ..BlendState::REPLACE
                    }
                };
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pipeline(
                        None,
                        Some(blend)
                    )))
                    .is_err()
                );
            }
        }
        for compare in [
            CompareFunction::Never,
            CompareFunction::Less,
            CompareFunction::Equal,
            CompareFunction::LessEqual,
            CompareFunction::Greater,
            CompareFunction::NotEqual,
            CompareFunction::GreaterEqual,
            CompareFunction::Always,
        ] {
            let pipeline = pipeline(Some(compare), None);
            for value in [0.25, 0.5, 0.75] {
                let passes = match compare {
                    CompareFunction::Never => false,
                    CompareFunction::Less => 0.5 < value,
                    CompareFunction::Equal => 0.5 == value,
                    CompareFunction::LessEqual => 0.5 <= value,
                    CompareFunction::Greater => 0.5 > value,
                    CompareFunction::NotEqual => 0.5 != value,
                    CompareFunction::GreaterEqual => 0.5 >= value,
                    CompareFunction::Always => true,
                };
                assert!(
                    render(&pipeline, Some(value))
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|pixel| pixel[0] == if passes { 255 } else { 0 }),
                    "{compare:?}, clear depth {value}"
                );
            }
        }
        for (blend, red, alpha) in [
            (
                BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Min,
                    },
                    ..BlendState::REPLACE
                },
                0i16,
                128i16,
            ),
            (
                BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Max,
                    },
                    ..BlendState::REPLACE
                },
                255,
                128,
            ),
            (BlendState::ALPHA_BLENDING, 128i16, 255i16),
            (BlendState::PREMULTIPLIED_ALPHA_BLENDING, 255, 255),
            (
                BlendState {
                    alpha: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        ..BlendComponent::OVER
                    },
                    ..BlendState::ALPHA_BLENDING
                },
                128,
                191,
            ),
            (
                BlendState {
                    color: BlendComponent {
                        operation: BlendOperation::ReverseSubtract,
                        ..BlendComponent::OVER
                    },
                    ..BlendState::REPLACE
                },
                0,
                128,
            ),
        ] {
            let pipeline = pipeline(None, Some(blend));
            assert!(
                render(&pipeline, None)
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| (i16::from(pixel[0]) - red).abs() <= 1
                        && pixel[1] == 0
                        && pixel[2] == 0
                        && (i16::from(pixel[3]) - alpha).abs() <= 1),
                "{blend:?}"
            );
        }
    }

    #[test]
    fn indexed_rendering_preserves_offsets_instances_and_vertex_formats() {
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
                struct In {
                    float4 position [[attribute(0)]];
                    float4 color [[attribute(1)]];
                    uint4 flags [[attribute(2)]];
                    int4 values [[attribute(3)]];
                };
                struct Out { float4 position [[position]]; float4 color; };
                vertex Out vs(In in [[stage_in]], uint instance [[instance_id]]) {
                    bool valid = all(in.flags == uint4(1, 2, 3, 4))
                        && all(in.values == int4(-1, -2, -3, -4)) && instance == 2;
                    return { in.position, valid ? in.color : float4(1, 0, 0, 1) };
                }
                fragment float4 fs(Out in [[stage_in]]) { return in.color; }
            "#,
            ),
        });
        let create_pipeline = |array_stride, attributes: &[VertexAttribute]| {
            device.create_render_pipeline(&RenderPipelineDescriptor {
                label: None,
                layout: None,
                vertex: VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(VertexBufferLayout {
                        array_stride,
                        step_mode: VertexStepMode::Vertex,
                        attributes,
                    })],
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
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = create_pipeline(
            36,
            &vertex_attr_array![0 => Float32x4, 1 => Unorm8x4, 2 => Uint8x4, 3 => Sint16x4],
        );
        for (stride, offset) in [(33, 0), (36, 1), (u64::from(u32::MAX) + 1, 0)] {
            let attributes = [VertexAttribute {
                format: VertexFormat::Float32x4,
                offset,
                shader_location: 0,
            }];
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| create_pipeline(
                    stride,
                    &attributes
                )))
                .is_err()
            );
        }
        let mut bytes: Vec<u8> = [[-1f32, -1., 0., 1.], [3., -1., 0., 1.], [-1., 3., 0., 1.]]
            .into_iter()
            .flat_map(|position| {
                position
                    .into_iter()
                    .flat_map(f32::to_ne_bytes)
                    .chain([0, 255, 0, 255, 1, 2, 3, 4])
                    .chain([-1i16, -2, -3, -4].into_iter().flat_map(i16::to_ne_bytes))
                    .chain([0; 4])
            })
            .collect();
        // Only the fetched attributes of the last vertex need to be present.
        bytes.truncate(bytes.len() - 4);
        let vertices = device.create_buffer(&BufferDescriptor {
            label: None,
            size: bytes.len() as u64,
            usage: BufferUsages::VERTEX,
            mapped_at_creation: true,
        });
        vertices
            .slice(..)
            .get_mapped_range_mut()
            .unwrap()
            .copy_from_slice(&bytes);
        vertices.unmap();
        let view = color.create_view(&Default::default());
        for format in [None, Some(IndexFormat::Uint16), Some(IndexFormat::Uint32)] {
            let indexed = format.is_some();
            let format = format.unwrap_or(IndexFormat::Uint16);
            let bytes: Vec<u8> = [99u32, 99, 4, 5, 6, 99]
                .into_iter()
                .flat_map(|index| match format {
                    IndexFormat::Uint16 => (index as u16).to_ne_bytes().to_vec(),
                    IndexFormat::Uint32 => index.to_ne_bytes().to_vec(),
                })
                .collect();
            let indices = device.create_buffer(&BufferDescriptor {
                label: None,
                size: bytes.len() as u64,
                usage: BufferUsages::INDEX,
                mapped_at_creation: true,
            });
            indices
                .slice(..)
                .get_mapped_range_mut()
                .unwrap()
                .copy_from_slice(&bytes);
            indices.unmap();
            let lifetime = Rc::downgrade(&indices.inner);
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(Color::BLACK),
                            store: StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                });
                pass.set_pipeline(&pipeline);
                use std::panic::{AssertUnwindSafe, catch_unwind};
                assert!(
                    catch_unwind(AssertUnwindSafe(
                        || pass.set_vertex_buffer(0, vertices.slice(1..))
                    ))
                    .is_err()
                );
                pass.set_vertex_buffer(0, vertices.slice(..));
                assert!(
                    catch_unwind(AssertUnwindSafe(
                        || pass.set_index_buffer(vertices.slice(..), format)
                    ))
                    .is_err()
                );
                assert!(
                    catch_unwind(AssertUnwindSafe(
                        || pass.set_index_buffer(indices.slice(1..), format)
                    ))
                    .is_err()
                );
                pass.set_index_buffer(
                    indices.slice(format.byte_size()..5 * format.byte_size()),
                    format,
                );
                assert!(
                    catch_unwind(AssertUnwindSafe(|| pass.draw_indexed(0..5, 0, 0..1))).is_err()
                );
                if indexed {
                    pass.draw_indexed(1..4, -4, 2..3);
                } else {
                    pass.draw(0..3, 2..3);
                }
            }
            drop(indices);
            assert!(lifetime.upgrade().is_some());
            queue.submit([encoder.finish()]);
            assert!(
                pixels(&device, &color)
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| *pixel == [0, 255, 0, 255])
            );
        }
    }

    #[test]
    fn invalid_upload_alignment_and_attachment_feedback_are_rejected() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let (device, _) = gpu();
        for (min, max) in [
            (f32::NAN, 1.0),
            (0.0, f32::INFINITY),
            (-1.0, 1.0),
            (2.0, 1.0),
        ] {
            assert!(
                catch_unwind(AssertUnwindSafe(|| device.create_sampler(
                    &SamplerDescriptor {
                        lod_min_clamp: min,
                        lod_max_clamp: max,
                        ..Default::default()
                    }
                )))
                .is_err()
            );
        }
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
