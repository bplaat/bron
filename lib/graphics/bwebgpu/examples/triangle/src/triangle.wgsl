/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Out {
    let positions = array(vec2(0.0, 0.75), vec2(-0.7, -0.6), vec2(0.7, -0.6));
    let colors = array(vec4(1.0, 0.1, 0.1, 1.0), vec4(0.1, 1.0, 0.2, 1.0), vec4(0.1, 0.3, 1.0, 1.0));
    var out: Out;
    out.position = vec4(positions[index], 0.0, 1.0);
    out.color = colors[index];
    return out;
}

@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return in.color;
}
