/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Standalone shader module; camera bindings match camera.rs.
struct Camera {
    eye: vec4<f32>, right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    screen_fog: vec4<f32>, environment: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;

fn corner_view(origin: vec3<f32>, corner: u32) -> vec3<f32> {
    let p = origin + vec3(f32(corner & 1u), f32((corner >> 1u) & 1u), f32(corner >> 2u)) * 1.006 - vec3(0.003);
    let relative = p - camera.eye.xyz;
    return vec3(dot(relative, camera.right.xyz) * camera.right.w,
        dot(relative, camera.up.xyz) * camera.up.w, dot(relative, camera.forward.xyz));
}

fn project(p: vec3<f32>) -> vec4<f32> {
    let near = camera.eye.w;
    let far = camera.forward.w;
    return vec4(p.xy, far * (p.z - near) / (far - near), p.z);
}

@vertex
fn vs_selection(@builtin(vertex_index) index: u32, @location(0) origin: vec3<f32>) -> @builtin(position) vec4<f32> {
    let edges = array(0u,1u, 2u,3u, 4u,5u, 6u,7u, 0u,2u, 1u,3u, 4u,6u, 5u,7u, 0u,4u, 1u,5u, 2u,6u, 3u,7u);
    let edge = index / 6u;
    var a = corner_view(origin, edges[edge * 2u]);
    var b = corner_view(origin, edges[edge * 2u + 1u]);
    let near = camera.eye.w;
    // Clip before expanding the line so edges near the eye cannot explode onscreen.
    if a.z < near && b.z < near { return vec4(2.0, 2.0, 1.0, 1.0); }
    if a.z < near { a = mix(a, b, (near - a.z) / (b.z - a.z)); }
    if b.z < near { b = mix(b, a, (near - b.z) / (a.z - b.z)); }
    let start = project(a);
    let end = project(b);
    let pixels = (end.xy / end.w - start.xy / start.w) * camera.screen_fog.xy * 0.5;
    let perpendicular = vec2(-pixels.y, pixels.x) / max(length(pixels), 0.001);
    let corners = array(vec2(0.0,-1.0), vec2(1.0,-1.0), vec2(1.0,1.0),
        vec2(0.0,-1.0), vec2(1.0,1.0), vec2(0.0,1.0));
    let corner = corners[index % 6u];
    var p = mix(start, end, corner.x);
    // Four-pixel-wide triangles work consistently across graphics backends.
    p = vec4(p.xy + perpendicular * corner.y * 4.0 / camera.screen_fog.xy * p.w, p.zw);
    return p;
}

@fragment
fn fs_selection() -> @location(0) vec4<f32> { return vec4(1.0); }
