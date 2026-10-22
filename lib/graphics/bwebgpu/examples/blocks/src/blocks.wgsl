/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

struct Scene {
    settings: vec4<f32>,
}
struct Face {
    position: vec4<f32>,
    data: vec4<u32>,
}

@group(0) @binding(0) var<uniform> scene: Scene;
@group(0) @binding(1) var materials: texture_2d_array<f32>;
@group(0) @binding(2) var material_sampler: sampler;
@group(0) @binding(3) var<storage, read> faces: array<Face>;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) shade: f32,
    @location(2) fog: f32,
    @interpolate(flat) @location(3) layer: u32,
    @interpolate(flat) @location(4) water: u32,
}

@vertex fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> Out {
    let data = faces[instance];
    let face = data.data.x;
    let corner = vertex;
    let horizontal = select(-0.5, 0.5, corner == 1u || corner == 2u);
    let vertical = select(-0.5, 0.5, corner >= 2u);
    var offset: vec3<f32>;
    var normal: vec3<f32>;
    switch face {
        case 0u: { offset = vec3(horizontal, vertical, 0.5); normal = vec3(0.0, 0.0, 1.0); }
        case 1u: { offset = vec3(-horizontal, vertical, -0.5); normal = vec3(0.0, 0.0, -1.0); }
        case 2u: { offset = vec3(0.5, vertical, -horizontal); normal = vec3(1.0, 0.0, 0.0); }
        case 3u: { offset = vec3(-0.5, vertical, horizontal); normal = vec3(-1.0, 0.0, 0.0); }
        case 4u: { offset = vec3(horizontal, 0.5, -vertical); normal = vec3(0.0, 1.0, 0.0); }
        default: { offset = vec3(horizontal, -0.5, vertical); normal = vec3(0.0, -1.0, 0.0); }
    }
    let world = data.position.xyz + vec3(0.5) + offset;
    let angle = scene.settings.x * 0.16;
    let center = vec3(32.0, 21.76, 32.0);
    // Preserve the desktop orbit, pulling back for the narrower portrait field of view.
    let camera_scale = 1.0 / min(scene.settings.y, 1.0);
    let eye = center + vec3(sin(angle) * 88.0, 58.0, cos(angle) * 88.0) * camera_scale;
    let forward = normalize(center - eye);
    let right = normalize(cross(forward, vec3(0.0, 1.0, 0.0)));
    let up = cross(right, forward);
    let relative = world - eye;
    let depth = dot(relative, forward);
    let focal = 2.1445069;
    let far = 500.0 * camera_scale;
    var out: Out;
    out.position = vec4(dot(relative, right) * focal / scene.settings.y,
        dot(relative, up) * focal, (far * depth - far) / (far - 1.0), depth);
    out.uv = vec2(select(0.0, 1.0, corner == 1u || corner == 2u), select(1.0, 0.0, corner >= 2u));
    out.shade = (0.42 + 0.58 * max(dot(normal, normalize(vec3(0.45, 0.78, 0.30))), 0.0)) * data.position.w / 255.0;
    out.fog = clamp((length(relative) / camera_scale - 120.0) / 180.0, 0.0, 1.0);
    out.layer = data.data.y;
    out.water = data.data.z;
    return out;
}

@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let color = textureSample(materials, material_sampler, in.uv, i32(in.layer));
    return vec4(mix(color.rgb * in.shade, vec3(0.18, 0.33, 0.62), in.fog), select(1.0, 0.6, in.water != 0u));
}
