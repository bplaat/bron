/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Layered Kenney skybox, or a blue/red background while immersed in water/lava.
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
@group(0) @binding(2) var sky_sampler: sampler;
@group(0) @binding(1) var sky_layers: texture_2d_array<f32>;

struct ScreenOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_screen(@builtin(vertex_index) index: u32) -> ScreenOutput {
    let points = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return ScreenOutput(vec4(points[index], 1.0, 1.0), points[index]);
}

// Half the size of the sun and moon on their tangent plane, tan(11 degrees).
const BODY_HALF_SIZE: f32 = 0.194;

fn over(color: vec3<f32>, layer: vec4<f32>) -> vec3<f32> {
    return mix(color, layer.rgb, layer.a);
}

fn cube_uv(direction: vec3<f32>) -> vec2<f32> {
    let axis = abs(direction);
    var uv: vec2<f32>;
    if axis.x >= axis.y && axis.x >= axis.z {
        uv = vec2(select(direction.z, -direction.z, direction.x > 0.0), -direction.y) / axis.x;
    } else if axis.y >= axis.z {
        uv = vec2(direction.x, select(-direction.z, direction.z, direction.y > 0.0)) / axis.y;
    } else {
        uv = vec2(select(-direction.x, direction.x, direction.z > 0.0), -direction.y) / axis.z;
    }
    return uv * 0.5 + 0.5;
}

fn sky(direction: vec3<f32>) -> vec3<f32> {
    let axis = abs(direction);
    let uv = cube_uv(direction);
    var color: vec3<f32>;
    if axis.y >= axis.x && axis.y >= axis.z {
        let layer = select(75, 79, direction.y > 0.0);
        color = textureSample(sky_layers, sky_sampler, uv, layer).rgb;
        if direction.y > 0.0 {
            let clouds = textureSample(
                sky_layers,
                sky_sampler,
                uv * 2.0 + camera.environment.y * vec2(0.0025, 0.00125),
                80,
            );
            color = over(color, clouds);
        }
    } else {
        color = textureSample(sky_layers, sky_sampler, uv, 76).rgb;
        color = over(color, textureSample(sky_layers, sky_sampler, uv, 78));
        color = over(color, textureSample(sky_layers, sky_sampler, uv, 77));
    }

    let night = color * vec3(0.06, 0.08, 0.16);
    color = mix(night, color, camera.environment.z);

    // The sun or moon is a square on the plane facing the viewer at its sky direction,
    // which keeps it square at any height instead of stretching with the latitude.
    let azimuth = select(-1.884956, 1.256637, camera.environment.z > 0.5);
    let elevation = 0.973894;
    let body = vec3(
        sin(azimuth) * cos(elevation),
        sin(elevation),
        cos(azimuth) * cos(elevation),
    );
    let facing = dot(direction, body);
    if facing > 0.0 {
        let right = normalize(cross(vec3(0.0, 1.0, 0.0), body));
        let up = cross(body, right);
        let local = vec2(dot(direction, right), dot(direction, up)) / facing;
        let sprite_uv = vec2(local.x, -local.y) / (2.0 * BODY_HALF_SIZE) + 0.5;
        if all(sprite_uv >= vec2(0.0)) && all(sprite_uv <= vec2(1.0)) {
            let layer = select(82, 81, camera.environment.z > 0.5);
            color = over(color, textureSample(sky_layers, sky_sampler, sprite_uv, layer));
        }
    }

    return color;
}

@fragment
fn fs_sky(in: ScreenOutput) -> @location(0) vec4<f32> {
    if camera.environment.x > 0.5 {
        let glow = select(
            vec3(0.025, 0.16, 0.23) * max(camera.environment.z, 0.06),
            vec3(0.65, 0.035, 0.005),
            camera.environment.x > 1.5,
        );
        return vec4(glow, 1.0);
    }
    let direction = normalize(camera.forward.xyz + camera.right.xyz * in.ndc.x / camera.right.w
        + camera.up.xyz * in.ndc.y / camera.up.w);
    return vec4(sky(direction), 1.0);
}
