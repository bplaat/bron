/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) height: f32,
}

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Out {
    let corners = array(vec2(-1.0, -3.0), vec2(-1.0, 1.0), vec2(3.0, 1.0));
    let corner = corners[index];
    var out: Out;
    out.position = vec4(corner, 1.0, 1.0);
    out.height = corner.y * 0.5 + 0.5;
    return out;
}

@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return vec4(mix(vec3(0.18, 0.33, 0.62), vec3(0.063, 0.1155, 0.217), clamp(in.height, 0.0, 1.0)), 1.0);
}
