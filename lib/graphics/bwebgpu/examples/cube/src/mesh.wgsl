/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

struct Camera { orbit: vec4<f32>, settings: vec4<f32> }

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var image_sampler: sampler;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

fn rotate(position: vec3<f32>) -> vec3<f32> {
    // Model rotation around X and Z precedes the fixed camera transform.
    let rx = camera.settings.z;
    let rz = camera.settings.w;
    let r = vec3(position.x, position.y*cos(rx)-position.z*sin(rx),
        position.y*sin(rx)+position.z*cos(rx));
    let p = vec3(r.x*cos(rz)-r.y*sin(rz), r.x*sin(rz)+r.y*cos(rz), r.z);
    let y = camera.orbit.x;
    let x = camera.orbit.y;
    let q = vec3(p.x*cos(y)+p.z*sin(y), p.y, -p.x*sin(y)+p.z*cos(y));
    return vec3(q.x, q.y*cos(x)-q.z*sin(x), q.y*sin(x)+q.z*cos(x));
}

@vertex fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> Out {
    var out: Out;
    let p = rotate(position) - vec3(0.0, 0.0, camera.orbit.z);
    let f = 2.41421356;
    out.position = vec4(p.x*f/camera.orbit.w, p.y*f, (-100.0*p.z-10.0)/99.9, -p.z);
    out.normal = rotate(normal);
    out.uv = uv;
    return out;
}

@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let color = textureSample(image, image_sampler, in.uv);
    if color.a < 0.1 { discard; }
    let light = 0.35 + 0.65 * abs(dot(normalize(in.normal), normalize(vec3(0.4, 0.8, 0.6))));
    return vec4(color.rgb * light, color.a);
}
