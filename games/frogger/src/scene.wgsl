/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex fn vs_main(@location(0) position: vec3<f32>, @location(1) color: vec4<f32>) -> Vertex {
    var out: Vertex;
    out.position = vec4(position, 1.0);
    out.color = color;
    return out;
}

@fragment fn fs_main(in: Vertex) -> @location(0) vec4<f32> {
    return in.color;
}
