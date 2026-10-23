/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Rotating selected-block preview; the material uses environment.w.
struct Camera {
    eye: vec4<f32>, right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>,
    screen_fog: vec4<f32>, environment: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var tiles: texture_2d_array<f32>;
@group(0) @binding(2) var tile_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) point: vec2<f32>,
}

@vertex
fn vs_screen(@builtin(vertex_index) index: u32) -> VertexOutput {
    let p = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return VertexOutput(vec4(p[index], 1.0, 1.0), p[index]);
}

fn rotate_x(p: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3(p.x, p.y * c - p.z * s, p.y * s + p.z * c);
}

fn rotate_y(p: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3(p.x * c + p.z * s, p.y, -p.x * s + p.z * c);
}

fn tile_layer(material: u32, normal: vec3<f32>) -> i32 {
    var layer = i32(material);
    if material == 1u {
        layer = select(1, 0, normal.y > 0.5);
        if normal.y < -0.5 { layer = 2; }
    }
    if material == 5u {
        layer = select(6, 5, normal.y > 0.5);
        if normal.y < -0.5 { layer = 2; }
    }
    if material == 6u { layer = select(7, 8, abs(normal.y) > 0.5); }
    if material == 7u { layer = select(9, 10, abs(normal.y) > 0.5); }
    if material == 8u { layer = select(65, 66, abs(normal.y) > 0.5); }
    if material >= 9u { layer = i32(material) + 2; }
    if material == 11u { layer = 11; }
    if material >= 12u { layer = i32(material) + 1; }
    if material == 22u { layer = 22; }
    if material == 24u { layer = 23; }
    if material == 25u {
        layer = select(24, 25, normal.y > 0.5);
        if normal.y < -0.5 { layer = 26; }
    }
    if material >= 26u { layer = i32(material) + 1; }
    if material == 21u { layer = 67; }
    return layer;
}

@fragment
fn fs_preview(in: VertexOutput) -> @location(0) vec4<f32> {
    let yaw = camera.environment.y * 1.8;
    let pitch = -0.48;
    var origin = vec3(0.0, 0.0, 3.2);
    // Leave enough projection margin for the widest corner-to-corner rotation.
    // A tighter field of view clipped corners and made the cube look squashed.
    var direction = normalize(vec3(in.point * 0.96, -2.2));
    origin = rotate_y(rotate_x(origin, -pitch), -yaw);
    direction = rotate_y(rotate_x(direction, -pitch), -yaw);

    let material = u32(camera.environment.w);
    if material >= 12u && material <= 15u {
        // Plants use the same two upright diagonal planes as their world mesh.
        // Test both planes so transparent texels on the front one can reveal
        // opaque texels on the plane behind it.
        var distance = 1e9;
        var color = vec4(0.0);
        let denominators = vec2(direction.z - direction.x, direction.z + direction.x);
        let numerators = vec2(origin.z - origin.x, origin.z + origin.x);
        for (var plane = 0u; plane < 2u; plane++) {
            if abs(denominators[plane]) > 0.0001 {
                let hit_distance = -numerators[plane] / denominators[plane];
                let point = origin + direction * hit_distance;
                if hit_distance > 0.0 && hit_distance < distance
                    && abs(point.x) <= 0.72 && abs(point.y) <= 0.72
                {
                    let horizontal = select(point.x, -point.x, plane == 1u);
                    let uv = vec2(horizontal / 1.44 + 0.5, 0.5 - point.y / 1.44);
                    let texel = textureSampleLevel(
                        tiles, tile_sampler, uv, i32(material) + 1, 0.0);
                    if texel.a >= 0.5 {
                        distance = hit_distance;
                        color = texel;
                    }
                }
            }
        }
        if distance == 1e9 { discard; }
        return vec4(color.rgb, 1.0);
    }

    let inverse = 1.0 / direction;
    let near3 = (-vec3(0.72) - origin) * inverse;
    let far3 = (vec3(0.72) - origin) * inverse;
    let entry3 = min(near3, far3);
    let exit3 = max(near3, far3);
    let entry = max(max(entry3.x, entry3.y), entry3.z);
    let exit = min(min(exit3.x, exit3.y), exit3.z);
    if entry > exit || exit < 0.0 { discard; }

    let point = origin + direction * max(entry, 0.0);
    let axis = abs(point);
    var normal = vec3(0.0);
    if axis.x > axis.y && axis.x > axis.z {
        normal.x = sign(point.x);
    } else if axis.y > axis.z {
        normal.y = sign(point.y);
    } else {
        normal.z = sign(point.z);
    }
    var uv = point.xy / 1.44 + 0.5;
    if abs(normal.x) > 0.5 { uv = vec2(point.z, point.y) / 1.44 + 0.5; }
    if abs(normal.y) > 0.5 { uv = point.xz / 1.44 + 0.5; }

    let texel = textureSample(tiles, tile_sampler, uv, tile_layer(material, normal));
    if material < 62u && texel.a < 0.5 { discard; }
    var color = texel.rgb;
    if material == 11u { color *= vec3(0.48, 0.68, 0.55); }
    let light = 0.64 + max(normal.y, 0.0) * 0.30 + max(normal.x, 0.0) * 0.12;
    return vec4(color * light, texel.a);
}
