/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// The minimum Android API level the bwindow and bwebview Android platforms support.
pub const MIN_SDK: u32 = 26;

// MARK: SDK
/// The adb of the SDK's platform-tools, or adb from the PATH when the SDK has none.
pub fn adb() -> PathBuf {
    sdk_dir()
        .ok()
        .map(|sdk| {
            sdk.join("platform-tools")
                .join(format!("adb{}", env::consts::EXE_SUFFIX))
        })
        .filter(|adb| adb.exists())
        .unwrap_or_else(|| PathBuf::from("adb"))
}

/// The Android SDK tools and the newest platform to compile against.
pub struct Sdk {
    /// The SDK directory.
    pub home: PathBuf,
    /// The newest build-tools directory, with aapt2, d8, zipalign, and apksigner.
    pub build_tools: PathBuf,
    /// The API level of the newest platform with an android.jar.
    pub platform: u32,
    /// The android.jar of that platform.
    pub platform_jar: PathBuf,
}

impl Sdk {
    /// Find the SDK through ANDROID_HOME or its default location.
    pub fn find() -> Result<Self, String> {
        let home = sdk_dir()?;
        if !home.is_dir() {
            return Err(format!(
                "Android SDK not found at {}, set ANDROID_HOME",
                home.display()
            ));
        }
        let build_tools = newest_version_dir(&home.join("build-tools")).ok_or(
            "Android SDK build-tools not found, install them with the SDK manager".to_string(),
        )?;
        let (platform, platform_jar) = newest_platform(&home).ok_or(
            "No Android SDK platform with an android.jar found, install one with the SDK manager"
                .to_string(),
        )?;
        Ok(Self {
            home,
            build_tools,
            platform,
            platform_jar,
        })
    }

    /// The bin directory of the NDK's LLVM toolchain, from ANDROID_NDK_HOME or the newest NDK.
    pub fn ndk_bin(&self) -> Result<PathBuf, String> {
        let ndk = env::var_os("ANDROID_NDK_HOME")
            .map(PathBuf::from)
            .or_else(|| newest_version_dir(&self.home.join("ndk")))
            .ok_or(
                "Android NDK not found, set ANDROID_NDK_HOME or install it with the SDK manager"
                    .to_string(),
            )?;
        let prebuilt = ndk.join("toolchains/llvm/prebuilt");
        fs::read_dir(&prebuilt)
            .ok()
            .and_then(|entries| {
                entries
                    .filter_map(|entry| Some(entry.ok()?.path().join("bin")))
                    .find(|bin| bin.is_dir())
            })
            .ok_or(format!(
                "Android NDK toolchain not found in {}",
                prebuilt.display()
            ))
    }
}

fn sdk_dir() -> Result<PathBuf, String> {
    if let Some(sdk) = env::var_os("ANDROID_HOME").or_else(|| env::var_os("ANDROID_SDK_ROOT")) {
        return Ok(PathBuf::from(sdk));
    }
    let home = env::home_dir().ok_or("Can't find the home directory".to_string())?;
    Ok(if cfg!(target_os = "macos") {
        home.join("Library/Android/sdk")
    } else {
        home.join("Android/Sdk")
    })
}

fn parse_version(name: &str) -> Option<Vec<u32>> {
    name.split('.').map(|part| part.parse().ok()).collect()
}

fn newest_version_dir(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let version = parse_version(entry.file_name().to_str()?)?;
            Some((version, entry.path()))
        })
        .max()
        .map(|(_, path)| path)
}

// Platform directories are named like android-36 or android-37.0.
fn platform_level(name: &str) -> Option<u32> {
    let version = name.strip_prefix("android-")?;
    version.split('.').next()?.parse().ok()
}

// The highest platform level that has an android.jar.
fn newest_platform(sdk: &Path) -> Option<(u32, PathBuf)> {
    fs::read_dir(sdk.join("platforms"))
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let jar = entry.path().join("android.jar");
            Some((platform_level(entry.file_name().to_str()?)?, jar))
                .filter(|(_, jar)| jar.exists())
        })
        .max()
}

// MARK: JDK
/// A JDK supported by the Android build tools.
pub struct Jdk {
    home: PathBuf,
}

impl Jdk {
    /// Use JAVA_HOME when it is JDK 17, else an installed JDK 17, else JDK 21.
    pub fn find() -> Result<Self, String> {
        let java_home = env::var_os("JAVA_HOME").map(PathBuf::from);
        let java_home_version = java_home.as_deref().and_then(major_version);
        [17, 21]
            .into_iter()
            .find_map(|version| {
                if java_home_version == Some(version) {
                    java_home.clone()
                } else {
                    find_jdk_home(version)
                }
            })
            .map(|home| Self { home })
            .ok_or("JDK 17 or 21 not found, install one or set JAVA_HOME".to_string())
    }

    /// A tool of this JDK, like javac or keytool.
    pub fn tool(&self, name: &str) -> PathBuf {
        self.home
            .join("bin")
            .join(format!("{name}{}", env::consts::EXE_SUFFIX))
    }

    /// A command for an SDK script like d8 or apksigner, which runs this JDK's java.
    pub fn command(&self, program: impl AsRef<OsStr>) -> Command {
        let mut paths = vec![self.home.join("bin")];
        paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
        let mut command = Command::new(program);
        command
            .env("JAVA_HOME", &self.home)
            .env("PATH", env::join_paths(paths).unwrap_or_default());
        command
    }
}

// java -version writes to stderr: openjdk version "17.0.1" ...
fn major_version(home: &Path) -> Option<u32> {
    let output = Command::new(home.join("bin/java"))
        .arg("-version")
        .output()
        .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version = stderr.split('"').nth(1)?;
    version.split(['.', '-', '+']).next()?.parse().ok()
}

fn find_jdk_home(version: u32) -> Option<PathBuf> {
    let has_java = |path: &Path| {
        path.join(format!("bin/java{}", env::consts::EXE_SUFFIX))
            .exists()
    };
    if cfg!(target_os = "macos") {
        // The java_home utility resolves installed JDKs by version
        let output = Command::new("/usr/libexec/java_home")
            .args(["-v", &version.to_string()])
            .output()
            .ok()?;
        let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        (output.status.success() && has_java(&path)).then_some(path)
    } else if cfg!(windows) {
        // Common JDK installation directories
        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        let prefixes = [
            format!("jdk-{version}"),
            format!("jdk{version}"),
            format!("zulu-{version}"),
        ];
        ["Eclipse Adoptium", "Microsoft", "Java", "Zulu"]
            .into_iter()
            .filter_map(|vendor| fs::read_dir(Path::new(&program_files).join(vendor)).ok())
            .flatten()
            .filter_map(Result::ok)
            .find(|entry| {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                prefixes.iter().any(|prefix| name.starts_with(prefix)) && has_java(&entry.path())
            })
            .map(|entry| entry.path())
    } else {
        // Common JDK paths on Debian/Ubuntu, RHEL/Fedora, and popular distros
        [
            format!("/usr/lib/jvm/java-{version}-openjdk-amd64"),
            format!("/usr/lib/jvm/java-{version}-openjdk-arm64"),
            format!("/usr/lib/jvm/java-{version}-openjdk"),
            format!("/usr/lib/jvm/temurin-{version}"),
            format!("/usr/lib/jvm/java-{version}"),
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| has_java(path))
    }
}

// MARK: Dex
/// Compile the Java sources in `java_dir` against the newest SDK platform into
/// `<out_dir>/classes.dex` and return its path, for build scripts to embed.
pub fn compile_dex(java_dir: &Path, out_dir: &Path, min_sdk: u32) -> Result<PathBuf, String> {
    for variable in ["ANDROID_HOME", "ANDROID_SDK_ROOT", "JAVA_HOME"] {
        println!("cargo::rerun-if-env-changed={variable}");
    }
    println!("cargo::rerun-if-changed={}", java_dir.display());
    let sdk = Sdk::find()?;
    let jdk = Jdk::find()?;

    let mut sources = Vec::new();
    collect_files(java_dir, "java", &mut sources);
    if sources.is_empty() {
        return Err(format!("No Java sources found in {}", java_dir.display()));
    }
    let classes_dir = out_dir.join("classes");
    if classes_dir.exists() {
        fs::remove_dir_all(&classes_dir).map_err(|error| error.to_string())?;
    }
    run(Command::new(jdk.tool("javac"))
        .args(["--release", "17", "-Xlint:all", "-Werror", "-classpath"])
        .arg(&sdk.platform_jar)
        .arg("-d")
        .arg(&classes_dir)
        .args(&sources))?;

    let mut classes = Vec::new();
    collect_files(&classes_dir, "class", &mut classes);
    run(jdk
        .command(sdk.build_tools.join("d8"))
        .args(["--release", "--min-api", &min_sdk.to_string(), "--lib"])
        .arg(&sdk.platform_jar)
        .arg("--output")
        .arg(out_dir)
        .args(&classes))?;
    Ok(out_dir.join("classes.dex"))
}

fn collect_files(dir: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .collect::<Vec<_>>();
    // Sorted so builds are reproducible.
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_files(&path, extension, files);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            files.push(path);
        }
    }
}

fn run(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .map_err(|error| format!("Failed to run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} failed with {status}"))
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_newest_versions_and_decimal_platforms() {
        let sdk = env::temp_dir().join(format!("android-build-sdk-{}", std::process::id()));
        for dir in [
            "build-tools/9.0.0",
            "build-tools/36.0.0",
            "build-tools/37.0.0",
            "build-tools/preview",
            "platforms/android-36",
            "platforms/android-37.0",
        ] {
            fs::create_dir_all(sdk.join(dir)).unwrap();
        }
        assert_eq!(
            newest_version_dir(&sdk.join("build-tools")),
            Some(sdk.join("build-tools/37.0.0"))
        );
        assert_eq!(newest_platform(&sdk), None);
        fs::write(sdk.join("platforms/android-36/android.jar"), []).unwrap();
        assert_eq!(
            newest_platform(&sdk),
            Some((36, sdk.join("platforms/android-36/android.jar")))
        );
        fs::write(sdk.join("platforms/android-37.0/android.jar"), []).unwrap();
        assert_eq!(
            newest_platform(&sdk),
            Some((37, sdk.join("platforms/android-37.0/android.jar")))
        );
        fs::remove_dir_all(sdk).unwrap();
    }
}
