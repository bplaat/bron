/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(1) var overlay: texture_2d<f32>;
@group(0) @binding(2) var overlay_sampler: sampler;

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Out {
    let uv = array(vec2(0.0, 0.0), vec2(0.0, 2.0), vec2(2.0, 0.0))[index];
    var out: Out;
    out.position = vec4(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return textureSample(overlay, overlay_sampler, in.uv);
}
