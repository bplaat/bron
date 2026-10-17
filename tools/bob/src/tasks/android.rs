/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::Path;
use std::process::{Command, exit};
use std::{env, fs};

use crate::executor::{ExecutorBuilder, TaskAction};
use crate::manifest::AndroidMetadata;
use crate::tasks::jvm::{
    find_jdk_home, find_modules, get_class_name, get_java_major_version, jdk_bin,
};
use crate::utils::{index_files, write_file_when_different};
use crate::version::Version;
use crate::{Bobje, Profile};

// MARK: Android vars
struct AndroidVars {
    id: String,
    android_metadata: AndroidMetadata,
    platform_jar: String,
}

impl AndroidVars {
    #[allow(unsafe_code)]
    fn new(bobje: &Bobje) -> Self {
        let id = bobje.manifest.package.id.as_ref().unwrap_or_else(|| {
            eprintln!("Manifest package id is required");
            exit(1);
        });
        let android_metadata = bobje
            .manifest
            .package
            .metadata
            .android
            .clone()
            .unwrap_or_default();
        let android_home = env::var("ANDROID_HOME").expect("$ANDROID_HOME env var must be set");
        let platform_jar = find_platform_jar(&android_home, android_metadata.target_sdk_version)
            .unwrap_or_else(|| {
                eprintln!(
                    "No platform found for target_sdk_version {} in {android_home}/platforms",
                    android_metadata.target_sdk_version
                );
                exit(1);
            });

        // Determine command_line_tools_path: prefer 'latest', else pick highest versioned folder
        let cmdline_tools_dir = format!("{android_home}/cmdline-tools");
        let latest_path = format!("{cmdline_tools_dir}/latest/bin");
        let command_line_tools_path = if Path::new(&latest_path).exists() {
            latest_path
        } else {
            // Find highest x.x versioned folder
            let mut highest_version: Option<(Version, String)> = None;
            if let Ok(entries) = fs::read_dir(&cmdline_tools_dir) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let file_name_str = file_name.to_string_lossy();
                    if let Ok(version) = Version::parse(&file_name_str)
                        && highest_version
                            .as_ref()
                            .is_none_or(|(highest_version, _)| &version > highest_version)
                    {
                        highest_version = Some((version, file_name_str.to_string()));
                    }
                }
            }
            if let Some((_, folder)) = highest_version {
                format!("{cmdline_tools_dir}/{folder}/bin")
            } else {
                eprintln!("No valid cmdline-tools found in {cmdline_tools_dir}");
                exit(1);
            }
        };

        // Find highest build-tools version matching the target_sdk_version
        let build_tools_dir = format!("{android_home}/build-tools");
        let mut highest_version: Option<Version> = None;
        if let Ok(entries) = fs::read_dir(&build_tools_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                if let Ok(version) = Version::parse(&file_name_str)
                    && version.major == android_metadata.target_sdk_version
                    && highest_version
                        .as_ref()
                        .is_none_or(|highest_version| &version > highest_version)
                {
                    highest_version = Some(version);
                }
            }
        }
        let build_tools_path = if let Some(version) = highest_version {
            format!("{build_tools_dir}/{version}")
        } else {
            eprintln!(
                "No build-tools found for target_sdk_version {} in {build_tools_dir}",
                android_metadata.target_sdk_version
            );
            exit(1);
        };

        // Platform tools path
        let platform_tools_path = format!("{android_home}/platform-tools");

        // For Android, ensure JAVA_HOME points to compatible JDK
        let overridden_java_home = find_android_java_home();
        if let Some(ref java_home) = overridden_java_home {
            // SAFETY: This runs during single-threaded setup, so there are no concurrent environment reads or writes anywhere in the process.
            unsafe { env::set_var("JAVA_HOME", java_home) };
        }

        // Extend current path
        let mut path =
            env::split_paths(&env::var("PATH").expect("Can't read $PATH")).collect::<Vec<_>>();
        path.insert(0, command_line_tools_path.into());
        path.insert(1, build_tools_path.into());
        path.insert(2, platform_tools_path.into());
        let new_path_str = env::join_paths(&path).expect("Can't join paths");
        // SAFETY: This runs during single-threaded setup, so there are no concurrent environment reads or writes anywhere in the process.
        unsafe { env::set_var("PATH", new_path_str) };

        Self {
            id: id.clone(),
            android_metadata,
            platform_jar,
        }
    }
}

fn find_platform_jar(android_home: &str, target_sdk_version: u16) -> Option<String> {
    [
        format!("{target_sdk_version}"),
        format!("{target_sdk_version}.0"),
    ]
    .into_iter()
    .map(|version| format!("{android_home}/platforms/android-{version}/android.jar"))
    .find(|path| Path::new(path).is_file())
}

// Returns a JDK 17 (or else JDK 21) JAVA_HOME for Android tooling, or `None` when the
// current JAVA_HOME is already JDK 17.
fn find_android_java_home() -> Option<String> {
    let version = get_java_major_version(&jdk_bin("java"))?;
    if version == 17 {
        return None;
    }
    eprintln!(
        "JAVA_HOME points to Java {version} (!= 17), searching for JDK 17 or 21 for Android..."
    );
    if let Some(home) = find_jdk_home(17) {
        return Some(home);
    }
    if let Some(home) = find_jdk_home(21) {
        eprintln!("JDK 17 not found, falling back to JDK 21");
        return Some(home);
    }
    eprintln!("Warning: could not find JDK 17 or 21, Android build may fail");
    None
}

pub(crate) fn detect_android(bobje: &Bobje) -> bool {
    Path::new(&format!("{}/AndroidManifest.xml", bobje.manifest_dir)).exists()
}

// MARK: Android resources
pub(crate) fn generate_android_res_tasks(bobje: &mut Bobje, executor: &mut ExecutorBuilder) {
    let vars = AndroidVars::new(bobje);

    let res_dir = format!(
        "{}/{}",
        bobje.manifest_dir, vars.android_metadata.resources_dir
    );
    if !Path::new(&res_dir).exists() {
        return;
    }

    // Compile resources task
    let compiled_res_dir = format!("{}/res/{}", bobje.out_dir(), vars.id.replace('.', "/"));
    for res_file in index_files(&res_dir) {
        let mut compiled_res_file = format!(
            "{}/{}.flat",
            compiled_res_dir,
            res_file
                .trim_start_matches(&res_dir)
                .trim_start_matches(['/', '\\'])
                .replace(['/', '\\'], "_")
        );
        if compiled_res_file.contains("/values") {
            compiled_res_file = compiled_res_file.replace(".xml", ".arsc");
        }
        executor.add_task_cmd(
            format!(
                "aapt2 compile {}{res_file} -o {compiled_res_dir}",
                if bobje.profile != Profile::Release && res_file.contains(".png") {
                    "--no-crunch "
                } else {
                    ""
                }
            ),
            vec![res_file],
            vec![compiled_res_file.clone()],
        );
    }

    // Link resources task
    let r_java_path = format!(
        "{}/src-gen/{}/R.java",
        bobje.out_dir(),
        vars.id.replace('.', "/")
    );
    if bobje.r#type.is_binary() {
        let dest = format!("{}/{}-unaligned.apk", bobje.out_dir(), bobje.name);

        let mut link_command = vec!["aapt2".to_string(), "link".to_string()];
        let mut link_inputs = vec![format!("{}/AndroidManifest.xml", bobje.manifest_dir)];
        let mut link_outputs = vec![dest.clone(), r_java_path.clone()];

        fn visit_bobje(
            bobje: &Bobje,
            link_command: &mut Vec<String>,
            link_inputs: &mut Vec<String>,
            link_outputs: &mut Vec<String>,
        ) {
            for dependency_bobje in bobje.dependencies.values() {
                visit_bobje(dependency_bobje, link_command, link_inputs, link_outputs);
            }
            if detect_android(bobje) {
                let android_metadata = bobje
                    .manifest
                    .package
                    .metadata
                    .android
                    .clone()
                    .unwrap_or_default();

                // Add assets
                let assets_dir = format!("{}/{}", bobje.manifest_dir, android_metadata.assets_dir);
                if fs::metadata(&assets_dir).is_ok() {
                    for asset in index_files(&assets_dir) {
                        link_inputs.push(asset.clone());
                    }
                }
                if fs::metadata(&assets_dir).is_ok() {
                    link_command.push("-A".to_string());
                    link_command.push(assets_dir.clone());
                }

                // Add compiled resources
                let package_id = bobje.manifest.package.id.as_ref().expect("Should be some");
                let compiled_res_dir =
                    format!("{}/res/{}", bobje.out_dir(), package_id.replace('.', "/"));
                let res_dir = format!("{}/{}", bobje.manifest_dir, android_metadata.resources_dir);
                if Path::new(&res_dir).exists() {
                    for res_file in index_files(&res_dir) {
                        let mut compiled_res_file = format!(
                            "{}/{}.flat",
                            compiled_res_dir,
                            res_file
                                .trim_start_matches(&res_dir)
                                .trim_start_matches(['/', '\\'])
                                .replace(['/', '\\'], "_")
                        );
                        if compiled_res_file.contains("/values") {
                            compiled_res_file = compiled_res_file.replace(".xml", ".arsc");
                        }
                        link_inputs.push(compiled_res_file);
                    }
                    if bobje.r#type.is_library() {
                        link_command.push("--extra-packages".to_string());
                        link_command.push(package_id.clone());
                        link_outputs.push(format!(
                            "{}/src-gen/{}/R.java",
                            bobje.out_dir(),
                            package_id.replace('.', "/")
                        ));
                    }
                    link_command.push(format!("{compiled_res_dir}/*.flat"));
                }
            }
        }
        visit_bobje(
            bobje,
            &mut link_command,
            &mut link_inputs,
            &mut link_outputs,
        );

        link_command.extend(vec![
            "--manifest".to_string(),
            format!("{}/AndroidManifest.xml", bobje.manifest_dir),
            "--java".to_string(),
            format!("{}/src-gen", bobje.out_dir()),
            "--version-name".to_string(),
            bobje.version.clone(),
            "--version-code".to_string(),
            parse_version_to_code(&bobje.version).to_string(),
            "--min-sdk-version".to_string(),
            vars.android_metadata.min_sdk_version.to_string(),
            "--target-sdk-version".to_string(),
            vars.android_metadata.target_sdk_version.to_string(),
            if bobje.profile != Profile::Release {
                "--debug-mode".to_string()
            } else {
                "".to_string()
            },
            "-I".to_string(),
            vars.platform_jar.clone(),
            "-o".to_string(),
            dest.to_string(),
        ]);
        executor.add_task_cmd(link_command.join(" "), link_inputs, link_outputs);
    }
    bobje.source_files.push(r_java_path.clone());
}

// MARK: Link classpath
pub(crate) fn link_android_classpath(bobje: &mut Bobje) {
    let vars = AndroidVars::new(bobje);
    bobje.manifest.build.classpath.push(vars.platform_jar);
}

// MARK: Android classes.dex
pub(crate) fn generate_android_dex_tasks(bobje: &Bobje, executor: &mut ExecutorBuilder) {
    let vars = AndroidVars::new(bobje);
    let classes_dir = format!("{}/classes", bobje.out_dir());
    let modules = find_modules(bobje);

    let mut inputs = modules
        .iter()
        .map(|module| format!("{}/{}", classes_dir, module.name.replace('.', "/")))
        .collect::<Vec<_>>();
    for dependency_bobje in bobje.dependencies.values() {
        if dependency_bobje.r#type.is_external_jar() {
            let jar = dependency_bobje.jar.as_ref().expect("Should be some");
            inputs.push(format!(
                "{}/{}",
                classes_dir,
                jar.package_override
                    .as_ref()
                    .unwrap_or(&jar.package)
                    .replace('.', "/")
            ));
        }
    }

    // Compile classes.dex with r8 task
    if bobje.profile == Profile::Release {
        // Write proguard config file
        let mut proguard_config = String::new();
        for module in &modules {
            for source_file in &module.source_files {
                if source_file.contains("Activity") {
                    proguard_config.push_str(&format!(
                        "-keep public class {} {{ protected void onCreate(android.os.Bundle); }}\n",
                        get_class_name(source_file)
                    ));
                }
            }
        }
        if let Some(android) = bobje.manifest.package.metadata.android.as_ref() {
            for keep in &android.proguard_keep {
                proguard_config.push_str(&format!("-keep {keep}\n"));
            }
        }
        let proguard_config_path = format!("{}/proguard.cfg", bobje.out_dir());
        write_file_when_different(&proguard_config_path, &proguard_config)
            .expect("Can't write proguard.cfg");

        // Add r8 task
        let r8_command = [
            "r8".to_string(),
            "--release".to_string(),
            "--dex".to_string(),
            format!("--min-api {}", vars.android_metadata.min_sdk_version),
            format!("--lib {}", vars.platform_jar),
            format!("--output {}", bobje.out_dir()),
            "--pg-compat".to_string(),
            format!("--pg-conf {proguard_config_path}"),
            format!("$(find {classes_dir} -name *.class)"),
        ];
        executor.add_task_cmd(
            r8_command.join(" "),
            inputs,
            vec![format!("{}/classes.dex", bobje.out_dir())],
        );
    }
    // Compile classes.dex with d8 task
    else {
        let d8_command = [
            "d8".to_string(),
            "--debug".to_string(),
            format!("--min-api {}", vars.android_metadata.min_sdk_version),
            format!("--lib {}", vars.platform_jar),
            format!("--output {}", bobje.out_dir()),
            format!("$(find {classes_dir} -name *.class)"),
        ];
        executor.add_task_cmd(
            d8_command.join(" "),
            inputs,
            vec![format!("{}/classes.dex", bobje.out_dir())],
        );
    }
}

// MARK: Android APK
pub(crate) fn generate_android_final_apk_tasks(bobje: &Bobje, executor: &mut ExecutorBuilder) {
    let vars = AndroidVars::new(bobje);

    // Copy dummy keystore if it doesn't exist
    let global_debug_key_store = env::home_dir()
        .expect("Can't find home dir")
        .join(".android")
        .join("debug.keystore");
    let keystore_configured = fs::metadata(&vars.android_metadata.keystore_file).is_ok();
    if !keystore_configured && fs::metadata(&global_debug_key_store).is_err() {
        fs::create_dir_all(
            global_debug_key_store
                .parent()
                .expect("Can't get parent dir"),
        )
        .expect("Can't create .android dir");

        let status = Command::new("keytool")
            .arg("-genkey")
            .arg("-v")
            .arg("-keystore")
            .arg(&global_debug_key_store)
            .arg("-storepass")
            .arg("android")
            .arg("-alias")
            .arg("androiddebugkey")
            .arg("-keypass")
            .arg("android")
            .arg("-dname")
            .arg("CN=Android Debug,O=Android,C=US")
            .arg("-keyalg")
            .arg("RSA")
            .arg("-keysize")
            .arg("2048")
            .arg("-validity")
            .arg("10000")
            .status()
            .expect("Failed to generate debug keystore");
        if !status.success() {
            exit(status.code().unwrap_or(1));
        }
    }

    // Add classes.dex to zip
    let unaligned_apk = format!("{}/{}-unaligned.apk", bobje.out_dir(), bobje.name);
    let apk_with_classes = format!("{}/{}-with-classes.apk", bobje.out_dir(), bobje.name);
    executor.add_task(
        TaskAction::AddDex(
            unaligned_apk.clone(),
            format!("{}/classes.dex", bobje.out_dir()),
            apk_with_classes.clone(),
        ),
        vec![
            unaligned_apk.clone(),
            format!("{}/classes.dex", bobje.out_dir()),
        ],
        vec![apk_with_classes.clone()],
    );

    // zipalign
    let unsigned_apk = format!("{}/{}-unsigned.apk", bobje.out_dir(), bobje.name);
    executor.add_task_cmd(
        format!("zipalign -f -p 4 {apk_with_classes} {unsigned_apk}"),
        vec![apk_with_classes.clone()],
        vec![unsigned_apk.clone()],
    );

    // apksigner
    let signed_apk = format!("{}/{}-{}.apk", bobje.out_dir(), bobje.name, bobje.version);
    let mut apksigner_cmd = format!(
        "apksigner sign --min-sdk-version {} --v4-signing-enabled false --ks {} ",
        vars.android_metadata.min_sdk_version,
        if keystore_configured {
            vars.android_metadata.keystore_file.clone()
        } else {
            global_debug_key_store.display().to_string()
        }
    );
    if keystore_configured {
        if !vars.android_metadata.key_alias.is_empty() {
            apksigner_cmd.push_str(&format!(
                "--ks-key-alias {} ",
                vars.android_metadata.key_alias
            ));
        }
        apksigner_cmd.push_str(&format!(
            "--ks-pass pass:{} --key-pass pass:{} ",
            vars.android_metadata.keystore_password, vars.android_metadata.key_password
        ));
    } else {
        apksigner_cmd.push_str(
            "--ks-key-alias androiddebugkey --ks-pass pass:android --key-pass pass:android ",
        );
    }
    apksigner_cmd.push_str(&format!("--in {unsigned_apk} --out {signed_apk}"));
    executor.add_task_cmd(
        apksigner_cmd,
        vec![unsigned_apk.clone()],
        vec![signed_apk.clone()],
    );
}

pub(crate) fn run_android_apk(bobje: &Bobje) -> ! {
    let vars = AndroidVars::new(bobje);

    // Try to install the APK
    let output = Command::new("adb")
        .arg("install")
        .arg("-r")
        .arg(format!(
            "{}/{}-{}.apk",
            bobje.out_dir(),
            bobje.name,
            bobje.version
        ))
        .output()
        .expect("Failed to execute adb");

    // When app signature doesn't match, uninstall and try again
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE") {
            Command::new("adb")
                .arg("uninstall")
                .arg(&vars.id)
                .status()
                .expect("Failed to execute adb");

            let status = Command::new("adb")
                .arg("install")
                .arg("-r")
                .arg(format!(
                    "{}/{}-{}.apk",
                    bobje.out_dir(),
                    bobje.name,
                    bobje.version
                ))
                .status()
                .expect("Failed to execute adb");
            if !status.success() {
                exit(status.code().unwrap_or(1));
            }
        } else {
            exit(output.status.code().unwrap_or(1));
        }
    }

    // Launch the Main Activity
    let status = Command::new("adb")
        .arg("shell")
        .arg("am")
        .arg("start")
        .arg("-n")
        .arg(format!(
            "{}/{}",
            vars.id, vars.android_metadata.main_activity
        ))
        .status()
        .expect("Failed to execute adb");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }

    // Clear logcat logs
    let status = Command::new("adb")
        .arg("logcat")
        .arg("-c")
        .status()
        .expect("Failed to execute adb logcat");
    if !status.success() {
        exit(status.code().unwrap_or(1));
    }

    // Start logcat with app filter
    let status = Command::new("adb")
        .arg("logcat")
        .arg(format!("{}:* *:S", bobje.name))
        .status()
        .expect("Failed to execute adb logcat");
    exit(status.code().unwrap_or(1))
}

// MARK: Utils
fn parse_version_to_code(version: &str) -> u32 {
    let version = Version::parse(version).expect("Can't parse numeric version");
    (version.major as u32) * 1_000_000 + (version.minor as u32) * 1_000 + (version.patch as u32)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_integer_and_decimal_platform_directories() {
        let directory = env::temp_dir().join(format!("bob-platform-test-{}", std::process::id()));
        let android_home = directory.to_str().expect("temporary directory path");
        fs::create_dir_all(&directory).expect("create temporary SDK");
        assert_eq!(find_platform_jar(android_home, 37), None);

        let decimal_platform = directory.join("platforms/android-37.0");
        fs::create_dir_all(&decimal_platform).expect("create decimal platform");
        let decimal_jar = decimal_platform.join("android.jar");
        fs::write(&decimal_jar, []).expect("create decimal platform jar");
        assert_eq!(
            find_platform_jar(android_home, 37)
                .as_deref()
                .map(Path::new),
            Some(decimal_jar.as_path())
        );
        assert_eq!(find_platform_jar(android_home, 36), None);

        let integer_platform = directory.join("platforms/android-37");
        fs::create_dir_all(&integer_platform).expect("create integer platform");
        let integer_jar = integer_platform.join("android.jar");
        fs::write(&integer_jar, []).expect("create integer platform jar");
        assert_eq!(
            find_platform_jar(android_home, 37)
                .as_deref()
                .map(Path::new),
            Some(integer_jar.as_path())
        );
        fs::remove_dir_all(directory).expect("remove temporary SDK");
    }
}
