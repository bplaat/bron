/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Inventory block icons drawn into slot viewports; the instance index is the material.
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
const TRANSLUCENT = 2u;

// Storage is only readable in the vertex stage, so it passes the material on.
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) point: vec2<f32>,
    @location(1) @interpolate(flat) tint: vec3<f32>,
    @location(2) @interpolate(flat) flags: u32,
    @location(3) @interpolate(flat) layers: vec4<u32>,
    @location(4) @interpolate(flat) more_layers: vec4<u32>,
}

@vertex
fn vs_screen(
    @builtin(vertex_index) index: u32,
    @builtin(instance_index) material: u32,
) -> VertexOutput {
    let p = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    let look = scene.materials[material];
    let l = look.layers;
    return VertexOutput(vec4(p[index], 1.0, 1.0), p[index], look.tint, look.flags,
        vec4(l[0], l[1], l[2], l[3]), vec4(l[4], l[5], l[6], l[7]));
}

fn layer(in: VertexOutput, face: u32) -> i32 {
    if face >= 4u { return i32(in.more_layers[face - 4u]); }
    return i32(in.layers[face]);
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

@fragment
fn fs_preview(in: VertexOutput) -> @location(0) vec4<f32> {
    // A fixed corner-on view, like Minecraft's inventory icons.
    let yaw = 0.785398;
    let pitch = 0.52;
    var origin = vec3(0.0, 0.0, 3.2);
    // Leave enough projection margin for the widest corner-to-corner rotation.
    // A tighter field of view clipped corners and made the cube look squashed.
    var direction = normalize(vec3(in.point * 0.96, -2.2));
    origin = rotate_y(rotate_x(origin, -pitch), -yaw);
    direction = rotate_y(rotate_x(direction, -pitch), -yaw);

    if (in.flags & SPRITE) != 0u {
        // Plants are flat sprites, like item icons. The derivative is taken
        // before the branch, because the material is not uniform for the analysis.
        let level = log2(max(length(dpdx(in.point)) * 64.0, 1.0));
        let uv = vec2(in.point.x, -in.point.y) * 0.5 + 0.5;
        let texel = textureSampleLevel(tiles, tile_sampler, uv, layer(in, 6u), level);
        if texel.a < 0.5 { discard; }
        return vec4(texel.rgb, 1.0);
    }

    let inverse = 1.0 / direction;
    let near3 = (-vec3(0.72) - origin) * inverse;
    let far3 = (vec3(0.72) - origin) * inverse;
    let entry3 = min(near3, far3);
    let exit3 = max(near3, far3);
    let entry = max(max(entry3.x, entry3.y), entry3.z);
    let exit = min(min(exit3.x, exit3.y), exit3.z);
    if entry > exit || exit < 0.0 { discard; }

    let point = origin + direction * max(entry, 0.0);
    let axis = abs(point);
    var normal = vec3(0.0);
    var face: u32;
    if axis.x > axis.y && axis.x > axis.z {
        normal.x = sign(point.x);
        face = select(0u, 1u, point.x < 0.0);
    } else if axis.y > axis.z {
        normal.y = sign(point.y);
        face = select(2u, 3u, point.y < 0.0);
    } else {
        normal.z = sign(point.z);
        face = select(4u, 5u, point.z < 0.0);
    }
    // Texture rows go down while y goes up, so side faces flip v to keep grass on top.
    var uv = vec2(point.x, -point.y) / 1.44 + 0.5;
    if abs(normal.x) > 0.5 { uv = vec2(point.z, -point.y) / 1.44 + 0.5; }
    if abs(normal.y) > 0.5 { uv = point.xz / 1.44 + 0.5; }

    let texel = textureSample(tiles, tile_sampler, uv, layer(in, face));
    if (in.flags & TRANSLUCENT) == 0u && texel.a < 0.5 { discard; }
    let color = texel.rgb * in.tint;
    // Bright top, lighter left and darker right sides, like Minecraft's block icons.
    var light = 0.62;
    if abs(normal.y) > 0.5 { light = 1.0; } else if abs(normal.x) > 0.5 { light = 0.8; }
    return vec4(color * light, texel.a);
}
