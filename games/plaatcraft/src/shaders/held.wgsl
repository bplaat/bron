/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The block in the player's hand in the bottom right of the view, which swings when the
// player breaks or places a block, like in Minecraft. The instance index holds the material
// in its low seven bits and the swing progress in 127ths above them.
// Camera bindings match camera.rs.
struct Camera {
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    screen_fog: vec4<f32>,
    environment: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var tiles: texture_2d_array<f32>;
@group(0) @binding(2) var tile_sampler: sampler;

// Per-material tint, flags and texture layers for the six faces and two sprite planes, followed
// by the chunk origins, see textures.rs.
struct Material {
    tint: vec3<f32>,
    flags: u32,
    layers: array<u32, 8>,
}
struct Scene {
    materials: array<Material, 128>,
    chunk_origins: array<vec4<f32>>,
}
@group(0) @binding(3) var<storage, read> scene: Scene;
const SPRITE = 1u;
const PI = 3.14159265;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) layer: i32,
    @location(2) @interpolate(flat) illumination: vec3<f32>,
}

// A box of a material's block from 0 to 1, see shape_flags in world.rs.
fn shape_box(flags: u32, index: u32) -> array<vec3<f32>, 2> {
    let bits = flags >> (17u + index * 6u);
    let low = vec3(f32(bits & 1u), f32((bits >> 1u) & 1u), f32((bits >> 2u) & 1u)) * 0.5;
    let high = 1.0 - vec3(f32((bits >> 3u) & 1u), f32((bits >> 4u) & 1u), f32((bits >> 5u) & 1u)) * 0.5;
    return array(low, high);
}

fn rotate_x(p: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3(p.x, p.y * c - p.z * s, p.y * s + p.z * c);
}

fn rotate_y(p: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3(p.x * c + p.z * s, p.y, -p.x * s + p.z * c);
}

@vertex
fn vs_held(
    @builtin(vertex_index) index: u32,
    @builtin(instance_index) instance: u32,
) -> VertexOutput {
    // Four corners of each face of the unit cube, counterclockwise seen from outside, in
    // the face order of the materials: +x, -x, +y, -y, +z, -z.
    let corners = array(
        vec3(1.0, 0.0, 0.0), vec3(1.0, 1.0, 0.0), vec3(1.0, 1.0, 1.0), vec3(1.0, 0.0, 1.0),
        vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 1.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0), vec3(0.0, 1.0, 1.0), vec3(1.0, 1.0, 1.0), vec3(1.0, 1.0, 0.0),
        vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(1.0, 0.0, 1.0), vec3(0.0, 0.0, 1.0),
        vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 1.0), vec3(1.0, 1.0, 1.0), vec3(0.0, 1.0, 1.0),
        vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(1.0, 1.0, 0.0),
    );
    let triangles = array(0u, 1u, 2u, 0u, 2u, 3u);
    let material = scene.materials[instance & 127u];
    let sprite = (material.flags & SPRITE) != 0u;
    // Plants are held as their front facing plane through the middle of the cube, slabs and
    // stairs as their boxes, 36 vertices each.
    var face = index / 6u % 6u;
    if sprite { face = 4u; }
    let bounds = shape_box(material.flags, index / 36u);
    var corner = mix(bounds[0], bounds[1], corners[face * 4u + triangles[index % 6u]]);
    var uv = vec2(corner.x, 1.0 - corner.y);
    if face < 2u { uv = vec2(corner.z, 1.0 - corner.y); }
    if face == 2u || face == 3u { uv = corner.xz; }
    var layer = material.layers[face];
    if sprite {
        corner.z = 0.5;
        layer = material.layers[6];
    }

    // The swing moves the block toward the middle and turns it down, like Minecraft's arm.
    let t = f32(instance >> 7u) / 127.0;
    let swing = sin(t * PI);
    let reach = sin(sqrt(t) * PI);
    var p = (corner - 0.5) * 0.4;
    p = rotate_y(p, 0.8 - swing * 0.35);
    p = rotate_x(p, 0.12 - reach * 1.2);
    // The view looks along -z with x to the right and y up.
    p += vec3(0.56 - reach * 0.4, -0.5 + sin(sqrt(t) * 2.0 * PI) * 0.2, -0.85 - swing * 0.2);

    let depth = -p.z;
    let near = camera.eye.w;
    let far = camera.forward.w;
    var out: VertexOutput;
    out.position = vec4(p.x * camera.right.w, p.y * camera.up.w,
        far * (depth - near) / (far - near), depth);
    out.uv = uv;
    out.layer = i32(layer);
    let face_light = array(0.8, 0.8, 1.0, 0.5, 0.65, 0.65)[face];
    let daylight = vec3(0.85, 0.92, 1.0) * camera.environment.z;
    out.illumination = max(vec3(0.06), daylight) * face_light * material.tint;
    return out;
}

@fragment
fn fs_held(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(tiles, tile_sampler, in.uv, in.layer);
    if texel.a < 0.5 { discard; }
    return vec4(texel.rgb * in.illumination, 1.0);
}
