/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// The other players: textured boxes for the head, body, arms, and legs, see the engine's avatars.
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

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) layer: i32,
    @location(3) @interpolate(flat) illumination: vec3<f32>,
}

// The box's x goes sideways, y up, and z along the facing direction.
@vertex
fn vs_player(
    @builtin(vertex_index) index: u32,
    @location(0) pivot: vec3<f32>,
    @location(1) rotation: vec3<f32>,
    @location(2) size: vec3<f32>,
    @location(3) offset: vec3<f32>,
) -> VertexOutput {
    // Four corners of each face of the unit cube, counterclockwise seen from outside.
    let corners = array(
        vec3(1.0, 0.0, 0.0), vec3(1.0, 1.0, 0.0), vec3(1.0, 1.0, 1.0), vec3(1.0, 0.0, 1.0),
        vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 1.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0), vec3(0.0, 1.0, 1.0), vec3(1.0, 1.0, 1.0), vec3(1.0, 1.0, 0.0),
        vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(1.0, 0.0, 1.0), vec3(0.0, 0.0, 1.0),
        vec3(0.0, 0.0, 1.0), vec3(1.0, 0.0, 1.0), vec3(1.0, 1.0, 1.0), vec3(0.0, 1.0, 1.0),
        vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(1.0, 1.0, 0.0),
    );
    let triangles = array(0u, 1u, 2u, 0u, 2u, 3u);
    let face = index / 6u;
    let corner = corners[face * 4u + triangles[index % 6u]];

    // The sprites are side views facing left, so the front is the left edge.
    let uv_inset = vec2(0.5 / 128.0);
    let uv = clamp(vec2(1.0 - corner.z, 1.0 - corner.y), uv_inset, 1.0 - uv_inset);

    // Pitch turns around the sideways axis, positive raises the front, then yaw turns
    // around the up axis like the camera.
    let local = offset + corner * size;
    let pitch = vec2(cos(rotation.y), sin(rotation.y));
    let pitched = vec3(local.x, local.y * pitch.x + local.z * pitch.y, local.z * pitch.x - local.y * pitch.y);
    let yaw = vec2(sin(rotation.x), cos(rotation.x));
    let right = vec3(yaw.y, 0.0, yaw.x);
    let forward = vec3(yaw.x, 0.0, -yaw.y);
    let p = pivot + right * pitched.x + vec3(0.0, pitched.y, 0.0) + forward * pitched.z;

    let relative = p - camera.eye.xyz;
    let depth = dot(relative, camera.forward.xyz);
    let near = camera.eye.w;
    let far = camera.forward.w;
    var out: VertexOutput;
    out.position = vec4(dot(relative, camera.right.xyz) * camera.right.w,
        dot(relative, camera.up.xyz) * camera.up.w, far * (depth - near) / (far - near), depth);
    out.world = p;
    out.uv = uv;
    out.layer = i32(rotation.z);
    let face_light = array(0.8, 0.8, 1.0, 0.5, 0.65, 0.65)[face];
    let daylight = vec3(0.85, 0.92, 1.0) * camera.environment.z;
    out.illumination = max(vec3(0.06), daylight) * face_light;
    return out;
}

@fragment
fn fs_player(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(tiles, tile_sampler, in.uv, in.layer);
    if texel.a < 0.5 { discard; }
    let shaded = texel.rgb * in.illumination;
    let relative = in.world - camera.eye.xyz;
    let fog = smoothstep(camera.screen_fog.z, camera.screen_fog.w, length(relative));
    let fog_color = mix(vec3(0.012, 0.018, 0.04), vec3(0.64, 0.79, 0.90), camera.environment.z);
    return vec4(mix(shaded, fog_color, fog), 1.0);
}
