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
    validate_bindings(path, &module)?;
    let (data, native_names) = match target {
        "macos" | "ios" => {
            let mut options = msl::Options {
                lang_version: (2, 1),
                fake_missing_bindings: false,
                ..Default::default()
            };
            let mut resources = msl::EntryPointResources {
                sizes_buffer: Some(30),
                ..Default::default()
            };
            for (_, global) in module.global_variables.iter() {
                let Some(binding) = global.binding else {
                    continue;
                };
                let slot = binding.binding as u8;
                let mut target = msl::BindTarget::default();
                match module.types[global.ty].inner {
                    naga::TypeInner::Image { .. } => target.texture = Some(slot),
                    naga::TypeInner::Sampler { .. } => {
                        target.sampler = Some(msl::BindSamplerTarget::Resource(slot));
                    }
                    _ => target.buffer = Some(slot),
                }
                resources.resources.insert(binding, target);
            }
            for entry in &module.entry_points {
                options
                    .per_entry_point_map
                    .insert(entry.name.clone(), resources.clone());
            }
            let (source, translation) = msl::write_string(
                &module,
                &info,
                &options,
                &msl::PipelineOptions {
                    vertex_pulling_transform: false,
                    ..Default::default()
                },
            )
            .map_err(|e| format!("{}: Metal translation: {e}", path.display()))?;
            let names = entry_names(path, "Metal", translation.entry_point_names)?;
            let metal_path = out.join(format!("{name}.metal"));
            write_if_changed(&metal_path, source.as_bytes())?;
            metal::compile(
                &metal_path,
                &out.join(format!("{name}.air")),
                &out.join(format!("{name}.metallib")),
                target,
            )?;
            let include = format!("/{name}.metallib");
            (
                format!(
                    "bwebgpu::ShaderSource::Binary(&[(\"\", include_bytes!(concat!(env!(\"OUT_DIR\"), {include:?})))])"
                ),
                names,
            )
        }
        "windows" => {
            let mut options = hlsl::Options {
                fake_missing_bindings: false,
                ..Default::default()
            };
            for (_, global) in module.global_variables.iter() {
                if let Some(binding) = global.binding {
                    options.binding_map.insert(
                        binding,
                        hlsl::BindTarget {
                            register: binding.binding,
                            ..Default::default()
                        },
                    );
                }
            }
            configure_hlsl(&mut options);
            let mut source = String::new();
            let translation = hlsl::Writer::new(&mut source, &options, &Default::default())
                .write(&module, &info, None)
                .map_err(|e| format!("{}: HLSL translation: {e}", path.display()))?;
            let source = single_sampler_tables(&source)
                .map_err(|e| format!("{}: HLSL sampler tables: {e}", path.display()))?;
            let names = entry_names(path, "HLSL", translation.entry_point_names)?;
            let entries = module
                .entry_points
                .iter()
                .zip(&names)
                .map(|(entry, native)| (entry.name.as_str(), native.as_str(), entry.stage))
                .collect::<Vec<_>>();
            let hlsl_path = out.join(format!("{name}.hlsl"));
            write_if_changed(&hlsl_path, source.as_bytes())?;
            let binaries = native_hlsl::compile(&hlsl_path, out, name, "dx12", "5_1", &entries)?;
            let values = binaries
                .iter()
                .map(|(entry, relative)| {
                    format!("({entry:?}, include_bytes!(concat!(env!(\"OUT_DIR\"), {relative:?})))")
                })
                .collect::<Vec<_>>()
                .join(",");
            (format!("bwebgpu::ShaderSource::Binary(&[{values}])"), names)
        }
        "linux" | "android" => {
            let spirv = spv::write_vec(&module, &info, &spv::Options::default(), None)
                .map_err(|e| format!("{}: SPIR-V translation: {e}", path.display()))?;
            let bytes: Vec<u8> = spirv.iter().flat_map(|word| word.to_le_bytes()).collect();
            write_if_changed(&out.join(format!("{name}.spv")), &bytes)?;
            (
                format!("bwebgpu::ShaderSource::SpirV(&{spirv:?})"),
                module
                    .entry_points
                    .iter()
                    .map(|entry| entry.name.clone())
                    .collect(),
            )
        }
        _ => return Err(format!("Unsupported graphics target: {target}").into()),
    };
    let entry_map = module
        .entry_points
        .iter()
        .zip(native_names)
        .map(|(entry, native)| format!("({:?}, {native:?})", entry.name))
        .collect::<Vec<_>>()
        .join(",");
    let descriptor = format!(
        "bwebgpu::ShaderModuleDescriptor {{ label: Some({name:?}), source: {data}, entries: &[{entry_map}] }}"
    );
    write_if_changed(&out.join(format!("{name}.rs")), descriptor.as_bytes())
}

fn entry_names<E: std::fmt::Display>(
    path: &Path,
    backend: &str,
    names: Vec<std::result::Result<String, E>>,
) -> Result<Vec<String>> {
    names
        .into_iter()
        .map(|name| {
            name.map_err(|e| format!("{}: {backend} entry point: {e}", path.display()).into())
        })
        .collect()
}

fn validate_bindings(path: &Path, module: &naga::Module) -> Result<()> {
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
    }
    Ok(())
}

fn configure_hlsl(options: &mut hlsl::Options) {
    // D3D system IDs exclude the draw's vertex and instance offsets.
    options.special_constants_binding = Some(hlsl::BindTarget {
        register: 5,
        ..Default::default()
    });
    // Naga requires a register for its sampler index buffer, which
    // single_sampler_tables replaces with a constant zero index.
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
    // Each draw binds a one-entry sampler table, so its sampler index is always zero.
    // Match only its generated declarations, leaving application arrays untouched.
    let output = source
        .replace(
            "SamplerState nagaSamplerHeap[2048]",
            "SamplerState nagaSamplerHeap[1]",
        )
        .replace(
            "SamplerComparisonState nagaComparisonSamplerHeap[2048]",
            "SamplerComparisonState nagaComparisonSamplerHeap[1]",
        )
        .replace(
            "StructuredBuffer<uint> nagaGroup0SamplerIndexArray : register(t4, space0);",
            "static const uint nagaGroup0SamplerIndexArray[1] = { 0 };",
        );
    for line in output.lines().map(str::trim) {
        if line.contains("SamplerIndexArray : register(") {
            return Err(format!("Unexpected generated sampler index buffer: {line}").into());
        }
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{configure_hlsl, single_sampler_tables};

    #[test]
    fn spirv_targets_only_generate_spirv_and_descriptor() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "bwebgpu-shader-build-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        fs::write(
            directory.join("shader.wgsl"),
            "@vertex fn main() -> @builtin(position) vec4<f32> { return vec4<f32>(0.0); }",
        )
        .expect("write shader");
        for target in ["linux", "android"] {
            let out = directory.join(target);
            crate::Builder::new()
                .shader("shader", "shader.wgsl")
                .compile_to(&directory, &out, target)
                .expect("compile SPIR-V shader");
            let mut files = fs::read_dir(&out)
                .expect("read shader outputs")
                .map(|entry| entry.expect("shader output").file_name())
                .collect::<Vec<_>>();
            files.sort();
            assert_eq!(files, ["shader.rs", "shader.spv"]);
            let spirv = fs::read(out.join("shader.spv")).expect("read SPIR-V");
            assert_eq!(spirv[..4], 0x0723_0203_u32.to_le_bytes());
            let descriptor = fs::read_to_string(out.join("shader.rs")).expect("read descriptor");
            assert!(descriptor.contains("ShaderSource::SpirV"));
            assert!(descriptor.contains("(\"main\", \"main\")"));
        }
        fs::remove_dir_all(directory).expect("remove test directory");
    }

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
        assert!(output.contains("static const uint nagaGroup0SamplerIndexArray[1] = { 0 };"));
        assert!(!output.contains("[2048]"));
        assert!(!output.contains("register(t4"));
    }

    #[test]
    fn unexpected_sampler_declarations_fail_at_build_time() {
        for source in [
            "SamplerState nagaSamplerHeap[4096]: register(s0, space0);",
            "SamplerState renamedHeap[2048]: register(s0, space0);",
            "SamplerComparisonState nagaComparisonSamplerHeap[]: register(s0, space1);",
            "StructuredBuffer<uint> nagaGroup0SamplerIndexArray : register(t5, space0);",
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
