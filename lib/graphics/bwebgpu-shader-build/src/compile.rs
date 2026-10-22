/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::Path;

use naga::ResourceBinding;
use naga::back::{hlsl, msl, spv};

use crate::output::write_if_changed;
use crate::{Result, hlsl as native_hlsl, metal};

pub(crate) fn compile_shader(path: &Path, out: &Path, name: &str, target: &str) -> Result<()> {
    let source =
        fs::read_to_string(path).map_err(|e| format!("{}: read WGSL: {e}", path.display()))?;
    let module = naga::front::wgsl::parse_str(&source).map_err(|e| {
        format!(
            "{}:\n{}",
            path.display(),
            e.emit_to_string_with_path(&source, &path.to_string_lossy())
        )
    })?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| {
        format!(
            "{}:\n{}",
            path.display(),
            e.emit_to_string_with_path(&source, &path.to_string_lossy())
        )
    })?;
    let mut msl_options = msl::Options {
        lang_version: (2, 1),
        fake_missing_bindings: false,
        ..Default::default()
    };
    let mut hlsl_options = hlsl::Options {
        fake_missing_bindings: false,
        ..Default::default()
    };
    let mut resources = msl::EntryPointResources {
        sizes_buffer: Some(30),
        ..Default::default()
    };
    configure_bindings(path, &module, &mut resources, &mut hlsl_options)?;
    for entry in &module.entry_points {
        msl_options
            .per_entry_point_map
            .insert(entry.name.clone(), resources.clone());
    }
    configure_hlsl(&mut hlsl_options);

    let (metal_source, metal_info) = msl::write_string(
        &module,
        &info,
        &msl_options,
        &msl::PipelineOptions {
            vertex_pulling_transform: false,
            ..Default::default()
        },
    )
    .map_err(|e| format!("{}: Metal translation: {e}", path.display()))?;
    let mut hlsl_source = String::new();
    let hlsl_info = hlsl::Writer::new(&mut hlsl_source, &hlsl_options, &Default::default())
        .write(&module, &info, None)
        .map_err(|e| format!("{}: HLSL translation: {e}", path.display()))?;
    let hlsl_source = single_sampler_tables(&hlsl_source)
        .map_err(|e| format!("{}: HLSL sampler tables: {e}", path.display()))?;
    let spirv = spv::write_vec(&module, &info, &spv::Options::default(), None)
        .map_err(|e| format!("{}: SPIR-V translation: {e}", path.display()))?;

    let mut entries = Vec::new();
    for (i, entry) in module.entry_points.iter().enumerate() {
        let metal_name = metal_info.entry_point_names[i]
            .as_ref()
            .map_err(|e| format!("{}: Metal entry point {}: {e}", path.display(), entry.name))?;
        let hlsl_name = hlsl_info.entry_point_names[i]
            .as_ref()
            .map_err(|e| format!("{}: HLSL entry point {}: {e}", path.display(), entry.name))?;
        let native = match target {
            "macos" | "ios" => metal_name,
            "windows" => hlsl_name,
            _ => &entry.name,
        };
        entries.push((entry.name.as_str(), native.as_str(), entry.stage));
    }
    write_outputs(
        out,
        name,
        target,
        &entries,
        &metal_source,
        &hlsl_source,
        &spirv,
    )
}

fn configure_bindings(
    path: &Path,
    module: &naga::Module,
    metal: &mut msl::EntryPointResources,
    hlsl: &mut hlsl::Options,
) -> Result<()> {
    for (_, global) in module.global_variables.iter() {
        let Some(binding) = global.binding else {
            continue;
        };
        if binding.group != 0 || binding.binding >= 16 {
            return Err(format!(
                "{}: binding {}:{} is outside group zero slots 0..16",
                path.display(),
                binding.group,
                binding.binding
            )
            .into());
        }
        let expected = match module.types[global.ty].inner {
            naga::TypeInner::Image { .. } => Some(1),
            naga::TypeInner::Sampler { .. } => Some(2),
            _ => match global.space {
                naga::AddressSpace::Uniform => Some(0),
                naga::AddressSpace::Storage { access } if access == naga::StorageAccess::LOAD => {
                    Some(3)
                }
                _ => None,
            },
        };
        if expected != Some(binding.binding) {
            return Err(format!(
                "{}: binding {}:{} is unsupported by the native backends",
                path.display(),
                binding.group,
                binding.binding
            )
            .into());
        }
        let slot = binding.binding as u8;
        let mut target = msl::BindTarget::default();
        match module.types[global.ty].inner {
            naga::TypeInner::Image { .. } => target.texture = Some(slot),
            naga::TypeInner::Sampler { .. } => {
                target.sampler = Some(msl::BindSamplerTarget::Resource(slot));
            }
            _ => target.buffer = Some(slot),
        }
        metal.resources.insert(binding, target);
        hlsl.binding_map.insert(
            binding,
            hlsl::BindTarget {
                register: binding.binding,
                ..Default::default()
            },
        );
    }
    Ok(())
}

fn configure_hlsl(options: &mut hlsl::Options) {
    // D3D system IDs exclude the draw's vertex and instance offsets.
    options.special_constants_binding = Some(hlsl::BindTarget {
        register: 5,
        ..Default::default()
    });
    // Each draw binds a one-entry sampler table and an index buffer containing zero.
    options.sampler_buffer_binding_map.insert(
        hlsl::SamplerIndexBufferKey { group: 0 },
        hlsl::BindTarget {
            register: 4,
            ..Default::default()
        },
    );
    if let Some(binding) = options.binding_map.get_mut(&ResourceBinding {
        group: 0,
        binding: 2,
    }) {
        binding.register = 0;
    }
}

fn single_sampler_tables(source: &str) -> Result<String> {
    // Naga 30 hardcodes these heap declarations to 2048, ignoring binding_array_size.
    // Match only its generated declarations, leaving application arrays untouched.
    let output = source
        .replace(
            "SamplerState nagaSamplerHeap[2048]",
            "SamplerState nagaSamplerHeap[1]",
        )
        .replace(
            "SamplerComparisonState nagaComparisonSamplerHeap[2048]",
            "SamplerComparisonState nagaComparisonSamplerHeap[1]",
        );
    for line in output.lines().map(str::trim) {
        for (kind, expected) in [
            ("SamplerState ", "SamplerState nagaSamplerHeap[1]:"),
            (
                "SamplerComparisonState ",
                "SamplerComparisonState nagaComparisonSamplerHeap[1]:",
            ),
        ] {
            if line.starts_with(kind) && !line.starts_with(expected) {
                return Err(format!("Unexpected generated sampler declaration: {line}").into());
            }
        }
    }
    Ok(output)
}

fn write_outputs(
    out: &Path,
    name: &str,
    target: &str,
    entries: &[(&str, &str, naga::ShaderStage)],
    metal_source: &str,
    hlsl: &str,
    spirv: &[u32],
) -> Result<()> {
    let metal_path = out.join(format!("{name}.metal"));
    let hlsl_path = out.join(format!("{name}.hlsl"));
    write_if_changed(&metal_path, metal_source.as_bytes())?;
    write_if_changed(&hlsl_path, hlsl.as_bytes())?;
    let data = match target {
        "macos" | "ios" => {
            let air_path = out.join(format!("{name}.air"));
            let library_path = out.join(format!("{name}.metallib"));
            metal::compile(&metal_path, &air_path, &library_path, target)?;
            let include = format!("/{name}.metallib");
            format!(
                "bwebgpu::ShaderSource::Binary(&[(\"\", include_bytes!(concat!(env!(\"OUT_DIR\"), {include:?})))])"
            )
        }
        "windows" => {
            let binaries = native_hlsl::compile(&hlsl_path, out, name, "dx12", "5_1", entries)?;
            let values = binaries
                .iter()
                .map(|(entry, relative)| {
                    format!("({entry:?}, include_bytes!(concat!(env!(\"OUT_DIR\"), {relative:?})))")
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("bwebgpu::ShaderSource::Binary(&[{values}])")
        }
        _ => format!("bwebgpu::ShaderSource::SpirV(&{spirv:?})"),
    };
    let entry_map = entries
        .iter()
        .map(|(original, native, _)| format!("({original:?}, {native:?})"))
        .collect::<Vec<_>>()
        .join(",");
    let descriptor = format!(
        "bwebgpu::ShaderModuleDescriptor {{ label: Some({name:?}), source: {data}, entries: &[{entry_map}] }}"
    );
    let bytes: Vec<u8> = spirv.iter().flat_map(|word| word.to_le_bytes()).collect();
    for (extension, contents) in [("spv", bytes.as_slice()), ("rs", descriptor.as_bytes())] {
        write_if_changed(&out.join(format!("{name}.{extension}")), contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{configure_hlsl, single_sampler_tables};

    #[test]
    fn d3d12_shader_ids_include_draw_offsets() {
        let module = naga::front::wgsl::parse_str(
            "@vertex fn main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> @builtin(position) vec4<f32> {\n\
                return vec4<f32>(f32(vertex), f32(instance), 0.0, 1.0);\n\
            }",
        ).expect("parse shader IDs");
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validate shader IDs");
        let mut options = naga::back::hlsl::Options::default();
        configure_hlsl(&mut options);
        let mut output = String::new();
        naga::back::hlsl::Writer::new(&mut output, &options, &Default::default())
            .write(&module, &info, None)
            .expect("translate shader IDs");
        assert!(output.contains("register(b5)"));
        assert!(output.contains("_NagaConstants.first_vertex + vertex"));
        assert!(output.contains("_NagaConstants.first_instance + instance"));
    }

    #[test]
    fn generated_sampler_table_fits_tier_one() {
        let module = naga::front::wgsl::parse_str(
            "@group(0) @binding(1) var image: texture_2d<f32>;\n\
             @group(0) @binding(2) var image_sampler: sampler;\n\
             @fragment fn main() -> @location(0) vec4<f32> {\n\
                 return textureSample(image, image_sampler, vec2<f32>(0.5));\n\
             }",
        )
        .expect("parse textured shader");
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validate textured shader");
        let mut options = naga::back::hlsl::Options {
            fake_missing_bindings: false,
            shader_model: naga::back::hlsl::ShaderModel::V5_1,
            ..Default::default()
        };
        for binding in [1, 2] {
            options.binding_map.insert(
                naga::ResourceBinding { group: 0, binding },
                naga::back::hlsl::BindTarget {
                    register: binding,
                    ..Default::default()
                },
            );
        }
        configure_hlsl(&mut options);
        let mut output = String::new();
        naga::back::hlsl::Writer::new(&mut output, &options, &Default::default())
            .write(&module, &info, None)
            .expect("translate textured shader");
        let output = single_sampler_tables(&output).expect("limit sampler tables");
        assert!(output.contains("SamplerState nagaSamplerHeap[1]: register(s0, space0)"));
        assert!(output.contains("nagaSamplerHeap[nagaGroup0SamplerIndexArray[0]]"));
        assert!(!output.contains("[2048]"));
    }

    #[test]
    fn unexpected_sampler_declarations_fail_at_build_time() {
        for source in [
            "SamplerState nagaSamplerHeap[4096]: register(s0, space0);",
            "SamplerState renamedHeap[2048]: register(s0, space0);",
            "SamplerComparisonState nagaComparisonSamplerHeap[]: register(s0, space1);",
        ] {
            assert!(single_sampler_tables(source).is_err());
        }
        let application_array = "static float weights[2048];";
        assert_eq!(
            single_sampler_tables(application_array).expect("ordinary array"),
            application_array
        );
    }

    #[test]
    fn d3d12_storage_buffer_uses_raw_byte_addressing() {
        let module = naga::front::wgsl::parse_str(
            "@group(0) @binding(3) var<storage, read> values: array<u32>;\n\
             @vertex fn main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {\n\
                 return vec4<f32>(f32(values[index]), 0.0, 0.0, 1.0);\n\
             }",
        )
        .expect("parse storage shader");
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("validate storage shader");
        let mut options = naga::back::hlsl::Options {
            fake_missing_bindings: false,
            shader_model: naga::back::hlsl::ShaderModel::V5_1,
            ..Default::default()
        };
        options.binding_map.insert(
            naga::ResourceBinding {
                group: 0,
                binding: 3,
            },
            naga::back::hlsl::BindTarget {
                register: 3,
                ..Default::default()
            },
        );
        configure_hlsl(&mut options);
        let mut output = String::new();
        naga::back::hlsl::Writer::new(&mut output, &options, &Default::default())
            .write(&module, &info, None)
            .expect("translate storage shader");
        assert!(output.contains("ByteAddressBuffer values : register(t3)"));
    }
}
