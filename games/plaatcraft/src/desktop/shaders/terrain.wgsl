/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Opaque terrain, emissive lava surfaces, and cutouts; includes water/lava immersion fog.
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
const EMISSIVE = 4u;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) @interpolate(flat) illumination: vec3<f32>,
    @location(5) @interpolate(flat) layer: i32,
    @location(6) occlusion: f32,
}

@vertex
fn vs_terrain(@builtin(vertex_index) index: u32,
    @location(0) packed: vec3<u32>) -> VertexOutput {
    let corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    let positive = array(0u, 1u, 2u, 0u, 2u, 3u);
    let negative = array(0u, 2u, 1u, 0u, 3u, 2u);
    let origin = vec3(f32(packed.x & 31u), f32((packed.x >> 5u) & 31u), f32((packed.x >> 12u) & 31u));
    let face = (packed.x >> 17u) & 7u;
    let material = ((packed.x >> 20u) & 63u) | ((packed.x >> 31u) << 6u);
    let extent = vec2(f32(packed.y & 31u), f32((packed.y >> 5u) & 31u));
    let chunk_origin = scene.chunk_origins[(packed.y >> 18u) | (((packed.x >> 10u) & 3u) << 14u)];
    let reverse = ((face & 1u) ^ ((packed.x >> 26u) & 1u)) != 0u;
    // Two bits of ambient occlusion per corner, split the quad along its brighter diagonal.
    let corner_light = array((packed.z & 3u), ((packed.z >> 2u) & 3u), ((packed.z >> 6u) & 3u),
        ((packed.z >> 4u) & 3u));
    let flip = select(0u, 1u, corner_light[0] + corner_light[2] < corner_light[1] + corner_light[3]);
    let corner_index = (select(positive[index], negative[index], reverse) + flip) % 4u;
    let corner = corners[corner_index];
    let axis = face / 2u;
    var p = origin + chunk_origin.xyz;
    p.y += f32((packed.x >> 27u) & 15u) * 16.0;
    var uv: vec2<f32>;
    if face >= 6u {
        p += vec3(corner.x, corner.y, select(corner.x, 1.0 - corner.x, face == 7u));
        uv = vec2(corner.x, 1.0 - corner.y);
    } else {
        var offset = corner * extent;
        var plane = 0.0;
        // A face of a box of slabs or stairs spans halves of its block, see world.rs.
        if (packed.z & 256u) != 0u {
            let low = vec2(f32((packed.z >> 9u) & 1u), f32((packed.z >> 11u) & 1u)) * 0.5;
            let high = 1.0 - vec2(f32((packed.z >> 10u) & 1u), f32((packed.z >> 12u) & 1u)) * 0.5;
            offset = mix(low, high, corner);
            plane = f32((packed.z >> 13u) & 3u) * 0.5;
        }
        if axis == 0u { p += vec3(plane, offset.x, offset.y); }
        if axis == 1u { p += vec3(offset.y, plane, offset.x); }
        if axis == 2u { p += vec3(offset.x, offset.y, plane); }
        uv = vec2(p.x, -p.y);
        if axis == 0u { uv = vec2(p.z, -p.y); }
        if axis == 1u { uv = p.xz; }
        // Flowing fluid's surface lies below the top of its block.
        let drop = f32((scene.materials[material].flags >> 8u) & 255u) / 255.0;
        if face == 2u || (axis == 0u && corner.x == 1.0) || (axis == 2u && corner.y == 1.0) {
            p.y -= drop;
        }
    }
    let relative = p - camera.eye.xyz;
    let depth = dot(relative, camera.forward.xyz);
    let near = camera.eye.w;
    let far = camera.forward.w;
    var out: VertexOutput;
    out.position = vec4(dot(relative, camera.right.xyz) * camera.right.w,
        dot(relative, camera.up.xyz) * camera.up.w, far * (depth - near) / (far - near), depth);
    out.world = p;
    out.uv = uv;
    let light = vec2(f32((packed.y >> 10u) & 15u), f32((packed.y >> 14u) & 15u)) / 15.0;
    let face_light = array(0.8, 0.8, 1.0, 0.5, 0.65, 0.65, 1.0, 1.0)[face];
    let skylight = vec3(0.85, 0.92, 1.0) * pow(light.x, 1.8) * camera.environment.z;
    let block_light = vec3(1.0, 0.62, 0.30) * pow(light.y, 1.8);
    out.illumination = max(vec3(0.018), max(skylight, block_light)) * face_light;
    if (scene.materials[material].flags & EMISSIVE) != 0u { out.illumination = vec3(1.0); }
    out.illumination *= scene.materials[material].tint;
    out.layer = i32(scene.materials[material].layers[face]);
    out.occlusion = array(0.45, 0.65, 0.82, 1.0)[corner_light[corner_index]];
    return out;
}

@fragment
fn fs_terrain(in: VertexOutput) -> @location(0) vec4<f32> {
    // Cutouts write depth only for visible texels, so grass needs no sorting.
    let texel = textureSample(tiles, tile_sampler, in.uv, in.layer);
    if texel.a < 0.5 { discard; }
    var color = texel.rgb;
    let relative = in.world - camera.eye.xyz;
    let fog = smoothstep(camera.screen_fog.z, camera.screen_fog.w, length(relative));
    let fog_color = mix(
        vec3(0.012, 0.018, 0.04),
        vec3(0.64, 0.79, 0.90),
        camera.environment.z,
    );
    let shaded = color * in.illumination * in.occlusion;
    var result = shaded;
    if fog > 0.0 && camera.environment.x < 0.5 {
        result = mix(shaded, fog_color, fog);
    }
    if camera.environment.x > 0.5 {
        let lava = camera.environment.x > 1.5;
        let tint = select(vec3(0.6, 0.85, 0.95), vec3(1.0, 0.3, 0.08), lava);
        let glow = select(vec3(0.025, 0.16, 0.23) * max(camera.environment.z, 0.06), vec3(0.65, 0.035, 0.005), lava);
        result = mix(shaded * tint, glow,
            1.0 - exp(-length(relative) * select(0.065, 0.35, lava)));
    }
    return vec4(result, 1.0);
}
