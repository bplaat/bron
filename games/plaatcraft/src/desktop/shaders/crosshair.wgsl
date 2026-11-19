/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Standalone shader module; camera bindings match camera.rs.
struct Camera {
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    screen_fog: vec4<f32>,
    environment: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;

@vertex
fn vs_screen(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(p[index], 1.0, 1.0);
}

@fragment
fn fs_crosshair(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = abs(position.xy - camera.screen_fog.xy * 0.5);
    let cross = (p.x < 2.0 && p.y < 12.0) || (p.y < 2.0 && p.x < 12.0);
    if !cross { discard; }
    return vec4(1.0);
}
