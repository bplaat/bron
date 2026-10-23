/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Translucent liquid surfaces, with water or lava immersion tint/fog at the camera.
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
@group(0) @binding(3) var<storage, read> chunk_origins: array<vec4<f32>>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(2) @interpolate(flat) material: u32,
    @location(3) uv: vec2<f32>,
    @location(4) @interpolate(flat) illumination: vec3<f32>,
}

@vertex
fn vs_liquid(@builtin(vertex_index) index: u32,
    @location(0) packed: vec2<u32>) -> VertexOutput {
    let corners = array(vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    let positive = array(0u, 1u, 2u, 0u, 2u, 3u);
    let negative = array(0u, 2u, 1u, 0u, 3u, 2u);
    let origin = vec3(f32(packed.x & 31u), f32((packed.x >> 5u) & 127u), f32((packed.x >> 12u) & 31u));
    let face = (packed.x >> 17u) & 7u;
    let material = (packed.x >> 20u) & 63u;
    let extent = vec2(f32(packed.y & 31u), f32((packed.y >> 5u) & 31u));
    let chunk_origin = chunk_origins[packed.y >> 18u];
    let reverse = (face & 1u) != 0u;
    let corner = corners[select(positive[index], negative[index], reverse)];
    let axis = face / 2u;
    var p = origin + chunk_origin.xyz;
    p.y += f32((packed.x >> 27u) & 15u) * 16.0;
    var uv: vec2<f32>;
    p[(axis + 1u) % 3u] += corner.x * extent.x;
    p[(axis + 2u) % 3u] += corner.y * extent.y;
    uv = vec2(p.x, -p.y);
    if axis == 0u { uv = vec2(p.z, -p.y); }
    if axis == 1u { uv = p.xz; }
    let relative = p - camera.eye.xyz;
    let depth = dot(relative, camera.forward.xyz);
    let near = camera.eye.w;
    let far = camera.forward.w;
    var out: VertexOutput;
    out.position = vec4(dot(relative, camera.right.xyz) * camera.right.w,
        dot(relative, camera.up.xyz) * camera.up.w, far * (depth - near) / (far - near), depth);
    out.world = p;
    out.material = material;
    out.uv = uv;
    let light = vec2(f32((packed.y >> 10u) & 15u), f32((packed.y >> 14u) & 15u)) / 15.0;
    let face_light = array(0.85, 0.85, 1.0, 0.65, 0.75, 0.75)[face];
    out.illumination = max(vec3(0.025), max(
        vec3(pow(light.x, 1.8) * camera.environment.z),
        vec3(1.0, 0.62, 0.30) * pow(light.y, 1.8))) * face_light;
    return out;
}

@fragment
fn fs_liquid(in: VertexOutput) -> @location(0) vec4<f32> {
    let relative = in.world - camera.eye.xyz;
    let fog = smoothstep(camera.screen_fog.z, camera.screen_fog.w, length(relative));
    let fog_color = mix(
        vec3(0.012, 0.018, 0.04),
        vec3(0.64, 0.79, 0.90),
        camera.environment.z,
    );
    let water = in.material == 22u;
    let uv = in.uv + select(
        vec2(0.0),
        camera.environment.y * vec2(0.0078125, 0.00390625),
        water,
    );
    let layer = select(i32(in.material) + 1, 22, water);
    let texel = textureSample(tiles, tile_sampler, uv, layer);
    // A gently drifting tile keeps water close to the block texture style.
    let color = select(
        texel.rgb,
        mix(vec3(0.035, 0.25, 0.38), texel.rgb * vec3(0.15, 0.58, 0.75), 0.75),
        water,
    );
    let shaded = color * in.illumination;
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
    return vec4(result, select(texel.a, mix(0.62, 1.0, fog), water));
}
