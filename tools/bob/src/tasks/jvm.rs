/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;
use std::process::{Command, exit};
use std::{env, fs};

use regex::{Regex, regex};
use sha1::Sha1;

use crate::Bobje;
use crate::args::Profile;
use crate::executor::{ExecutorBuilder, TaskAction};
use crate::manifest::JarDependency;
use crate::utils::cache_dir;

const CLASSPATH_SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };

// MARK: Java/Kotlin tasks
pub(crate) fn detect_java_kotlin(source_files: &[String]) -> bool {
    detect_java(source_files) || detect_kotlin(source_files)
}

pub(crate) fn detect_java(source_files: &[String]) -> bool {
    source_files.iter().any(|path| path.ends_with(".java"))
}

pub(crate) fn detect_kotlin(source_files: &[String]) -> bool {
    source_files.iter().any(|path| path.ends_with(".kt"))
}

pub(crate) fn generate_javac_kotlinc_tasks(bobje: &Bobje, executor: &mut ExecutorBuilder) {
    let classes_dir = format!("{}/classes", bobje.out_dir());
    fs::create_dir_all(&classes_dir).expect("Failed to create classes directory");

    let modules = find_modules(bobje);
    let module_deps = find_dependencies(&modules);

    let javac_bin = jdk_bin("javac");

    let mut javac_flags = "-Xlint -Werror".to_string();
    if bobje.profile == Profile::Debug {
        javac_flags.push_str(" -g");
    }
    if !bobje.manifest.build.javac_flags.is_empty() {
        javac_flags.push(' ');
        javac_flags.push_str(&bobje.manifest.build.javac_flags);
    }

    let mut kotlinc_flags = "-no-reflect -Werror".to_string();
    if !bobje.manifest.build.kotlinc_flags.is_empty() {
        kotlinc_flags.push(' ');
        kotlinc_flags.push_str(&bobje.manifest.build.kotlinc_flags);
    }

    let mut classpath = String::new();
    classpath.push_str(&classes_dir);

    if !bobje.manifest.build.classpath.is_empty() {
        for path in &bobje.manifest.build.classpath {
            if !Path::new(path).exists() {
                eprintln!("Classpath entry '{path}' does not exist");
                exit(1);
            }
            classpath.push_str(CLASSPATH_SEPARATOR);
            classpath.push_str(path);
        }
    }

    for module in &modules {
        let mut inputs = module.source_files.clone();
        if let Some(dependencies) = module_deps.get(&module.name) {
            for dependency_module in dependencies {
                let classes_module_dir = format!(
                    "{}/{}",
                    classes_dir,
                    dependency_module.name.replace('.', "/")
                );
                if !inputs.contains(&classes_module_dir) {
                    inputs.push(classes_module_dir);
                }
            }
        }
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

        let mut actions = Vec::new();

        // Javac
        let java_files = module
            .source_files
            .iter()
            .filter(|f| f.ends_with(".java"))
            .map(|f| f.to_string())
            .collect::<Vec<_>>();
        if !java_files.is_empty() {
            actions.push(TaskAction::Command(format!(
                "{javac_bin} {} -cp {} -d {} {}",
                javac_flags,
                classpath,
                classes_dir,
                java_files.join(" ")
            )));
        }

        // Kotlinc
        let kotlin_files = module
            .source_files
            .iter()
            .filter(|f| f.ends_with(".kt"))
            .cloned()
            .collect::<Vec<_>>();
        if !kotlin_files.is_empty() {
            actions.push(TaskAction::Command(format!(
                "kotlinc {} -cp {} -d {} {}",
                kotlinc_flags,
                classpath,
                classes_dir,
                kotlin_files.join(" ")
            )));
        }

        executor.add_task(
            TaskAction::Multiple(actions),
            inputs,
            vec![format!("{}/{}", classes_dir, module.name.replace('.', "/"))],
        );
    }

    // Add phony build target with all tests
    if bobje.profile == Profile::Test {
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for module in &modules {
            let mut found_test = false;
            for source_file in &module.source_files {
                if source_file.ends_with("Test.java") || source_file.ends_with("Test.kt") {
                    found_test = true;
                    outputs.push(format!(
                        "{}/{}.class",
                        classes_dir,
                        get_class_name(source_file).replace(".", "/")
                    ));
                }
            }
            if found_test {
                inputs.push(format!("{}/{}", classes_dir, module.name.replace('.', "/")));
            }
        }
        executor.add_task_phony(inputs, outputs);
    }
}

pub(crate) fn run_java_class(bobje: &Bobje) -> ! {
    let java_bin = jdk_bin("java");
    let status = Command::new(&java_bin)
        .arg("-cp")
        .arg(format!("{}/classes", bobje.out_dir()))
        .arg(find_main_class(bobje).unwrap_or_else(|| {
            eprintln!("Can't find main class");
            exit(1);
        }))
        .status()
        .expect("Failed to execute java");
    exit(status.code().unwrap_or(1))
}

pub(crate) fn run_junit_tests(bobje: &Bobje) -> ! {
    let java_bin = jdk_bin("java");
    let mut cmd = Command::new(&java_bin);
    cmd.arg("-cp")
        .arg(format!("{}/classes", bobje.out_dir()))
        .arg("org.junit.runner.JUnitCore");
    for source_file in &bobje.source_files {
        if source_file.ends_with("Test.java") || source_file.ends_with("Test.kt") {
            cmd.arg(get_class_name(source_file));
        }
    }

    let status = cmd.status().expect("JUnit tests failed");
    exit(status.code().unwrap_or(1))
}

// MARK: Jar tasks
pub(crate) const fn detect_jar(bobje: &Bobje) -> bool {
    bobje.manifest.package.metadata.jar.is_some()
}

pub(crate) fn download_extract_jar_tasks(
    bobje: &Bobje,
    executor: &mut ExecutorBuilder,
    jar: &JarDependency,
) {
    // Add download task
    let cache_dir = cache_dir();
    let source = jar.url.as_deref().or(jar.path.as_deref()).unwrap_or("");
    let mut cache_key = String::new();
    for byte in Sha1::digest(source.as_bytes()) {
        write!(&mut cache_key, "{byte:02x}").expect("write cache key");
    }
    let downloaded_jar = format!(
        "{}/jar-cache/{cache_key}-{}-{}.jar",
        cache_dir.display(),
        bobje.name,
        bobje.version
    );
    let lock_file = format!("{downloaded_jar}.lock");
    if let Some(path) = &jar.path {
        executor.add_task_cp(path.clone(), downloaded_jar.clone());
    }
    if let Some(url) = &jar.url {
        executor.add_task(
            TaskAction::Download(url.clone(), downloaded_jar.clone(), lock_file),
            vec![],
            vec![downloaded_jar.clone()],
        );
    }

    // Add extract task
    let classes_dir = format!("{}/classes", bobje.out_dir());
    executor.add_task(
        TaskAction::ExtractJar(downloaded_jar.clone(), classes_dir.clone()),
        vec![downloaded_jar],
        vec![format!(
            "{}/{}",
            classes_dir,
            jar.package_override
                .as_ref()
                .unwrap_or(&jar.package)
                .replace('.', "/")
        )],
    );
}

pub(crate) fn generate_jar_tasks(bobje: &Bobje, executor: &mut ExecutorBuilder) {
    let classes_dir = format!("{}/classes", bobje.out_dir());
    let modules = find_modules(bobje);

    let main_class = bobje
        .manifest
        .package
        .metadata
        .jar
        .as_ref()
        .and_then(|jar| jar.main_class.clone())
        .unwrap_or_else(|| {
            find_main_class(bobje).unwrap_or_else(|| {
                eprintln!("Can't find main class");
                exit(1);
            })
        });

    // Minify names and tree shake classes with ProGuard
    let optimized_classes_dir = format!("{classes_dir}-optimized");
    if bobje.profile == Profile::Release {
        let java_home = env::var("JAVA_HOME").expect("$JAVA_HOME not set");
        let mut keeps = vec![format!(
            "public class {} {{ public static void main(java.lang.String[]); }}",
            main_class
        )];
        if let Some(jar) = bobje.manifest.package.metadata.jar.as_ref() {
            keeps.extend(jar.proguard_keep.clone());
        }

        executor.add_task_cmd(
            format!(
                "proguard -injars {}\\(!META-INF/**\\) -outjars {} -libraryjars {}/jmods/java.base.jmod {} > /dev/null",
                classes_dir, optimized_classes_dir, java_home,
                keeps
                    .iter()
                    .map(|keep| format!("-keep '{keep}'"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            modules
                .iter()
                .map(|module| format!("{}/{}", classes_dir, module.name.replace('.', "/")))
                .collect::<Vec<_>>(),
            vec![optimized_classes_dir.clone()],
        );
    }

    // Build JAR file
    let jar_bin = jdk_bin("jar");
    let jar_file = format!("{}/{}-{}.jar", bobje.out_dir(), bobje.name, bobje.version);
    executor.add_task_cmd(
        format!(
            "{jar_bin} cfe {} {} -C {} .",
            jar_file,
            main_class,
            if bobje.profile == Profile::Release {
                &optimized_classes_dir
            } else {
                &classes_dir
            }
        ),
        if bobje.profile == Profile::Release {
            vec![optimized_classes_dir]
        } else {
            modules
                .iter()
                .map(|module| format!("{}/{}", classes_dir, module.name.replace('.', "/")))
                .collect::<Vec<_>>()
        },
        vec![jar_file],
    );
}

pub(crate) fn run_jar(bobje: &Bobje) -> ! {
    let status = Command::new("java")
        .arg("-jar")
        .arg(format!(
            "{}/{}-{}.jar",
            bobje.out_dir(),
            bobje.name,
            bobje.version
        ))
        .status()
        .expect("Failed to execute java");
    exit(status.code().unwrap_or(1))
}

// MARK: Utils
pub(crate) fn jdk_bin(bin: &str) -> String {
    env::var("JAVA_HOME")
        .map(|home| format!("{home}/bin/{bin}{}", env::consts::EXE_SUFFIX))
        .unwrap_or_else(|_| bin.to_string())
}

pub(crate) fn get_java_major_version(java_bin: &str) -> Option<u32> {
    // java -version writes to stderr: openjdk version "17.0.1" ...
    let output = Command::new(java_bin).arg("-version").output().ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let re = regex!(r#"version "(\d+)"#);
    let caps = re.captures(&stderr)?;
    caps[1].parse().ok()
}

pub(crate) fn find_jdk_home(version: u32) -> Option<String> {
    let v = version.to_string();
    if cfg!(target_os = "macos") {
        // macOS: the java_home utility resolves installed JDKs by version
        if let Ok(output) = Command::new("/usr/libexec/java_home")
            .args(["-v", &v])
            .output()
            && output.status.success()
        {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    } else if cfg!(target_os = "linux") {
        // Common JDK paths on Debian/Ubuntu, RHEL/Fedora, and popular distros
        let candidates = [
            format!("/usr/lib/jvm/java-{v}-openjdk-amd64"),
            format!("/usr/lib/jvm/java-{v}-openjdk-arm64"),
            format!("/usr/lib/jvm/java-{v}-openjdk"),
            format!("/usr/lib/jvm/temurin-{v}"),
            format!("/usr/lib/jvm/java-{v}"),
        ];
        for path in &candidates {
            if Path::new(path).join("bin/java").exists() {
                return Some(path.clone());
            }
        }
        // Fallback: parse update-java-alternatives (Debian/Ubuntu)
        if let Ok(output) = Command::new("update-java-alternatives")
            .arg("--list")
            .output()
        {
            let java_tag = format!("java-{v}");
            let temurin_tag = format!("temurin-{v}");
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if (line.contains(&java_tag) || line.contains(&temurin_tag))
                    && let Some(path) = line.split_whitespace().last()
                    && Path::new(path).join("bin/java").exists()
                {
                    return Some(path.to_string());
                }
            }
        }
    } else if cfg!(windows) {
        // Common JDK installation directories on Windows
        let program_files =
            env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string());
        for vendor in &["Eclipse Adoptium", "Microsoft", "Java", "Zulu"] {
            let base = format!(r"{program_files}\{vendor}");
            if let Ok(entries) = fs::read_dir(&base) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_lower = name.to_string_lossy().to_lowercase();
                    if name_lower.starts_with(&format!("jdk-{v}"))
                        || name_lower.starts_with(&format!("jdk{v}"))
                        || name_lower.starts_with(&format!("zulu-{v}"))
                    {
                        let path = entry.path();
                        if path
                            .join(format!("bin/java{}", env::consts::EXE_SUFFIX))
                            .exists()
                        {
                            return Some(path.display().to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn get_class_name(source_file: &str) -> String {
    let relative_path = source_file
        .split("src/")
        .nth(1)
        .or_else(|| source_file.split("src-gen/").nth(1))
        .unwrap_or(source_file);

    relative_path
        .trim_end_matches(".java")
        .trim_end_matches(".kt")
        .replace(['/', '\\'], ".")
}

fn get_module_name(source_file: &str) -> String {
    get_class_name(source_file)
        .rsplit_once('.')
        .map_or("", |(prefix, _)| prefix)
        .to_string()
}

#[derive(Clone)]
pub(crate) struct Module {
    pub name: String,
    pub source_files: Vec<String>,
}

pub(crate) fn find_modules(bobje: &Bobje) -> Vec<Module> {
    let mut modules = Vec::new();
    for dependency_bobje in bobje.dependencies.values() {
        for module in find_modules(dependency_bobje) {
            modules.push(module);
        }
    }

    for source_file in &bobje.source_files {
        if source_file.ends_with(".java") || source_file.ends_with(".kt") {
            if bobje.profile != Profile::Test
                && (source_file.ends_with("Test.java") || source_file.ends_with("Test.kt"))
            {
                continue;
            }

            let module_name = get_module_name(source_file);
            if let Some(module) = modules.iter_mut().find(|m| m.name == module_name) {
                module.source_files.push(source_file.to_string());
            } else {
                modules.push(Module {
                    name: module_name.clone(),
                    source_files: vec![source_file.to_string()],
                });
            }
        }
    }

    // Sort module with main class to be last
    if let Some(main_class) = find_main_class(bobje)
        && let Some(pos) = modules
            .iter()
            .position(|m| m.name == get_module_name(&main_class))
    {
        let main_module = modules.remove(pos);
        modules.push(main_module);
    }

    modules
}

fn find_dependencies(modules: &Vec<Module>) -> HashMap<String, Vec<Module>> {
    let mut module_deps = HashMap::new();
    for module in modules {
        for source_file in &module.source_files {
            if let Ok(contents) = fs::read_to_string(source_file) {
                for other_module in modules {
                    if other_module.name == module.name {
                        continue;
                    }
                    let re = Regex::new(&format!(
                        r"import\s+(?:static\s+)?{}\s*\.\s*(?:\w+|\*)\s*[;\n\r]",
                        other_module.name
                    ))
                    .expect("Failed to compile regex");
                    if re.is_match(&contents) {
                        module_deps
                            .entry(module.name.clone())
                            .or_insert_with(Vec::new)
                            .push(other_module.clone());
                    }
                }
            }
        }
    }
    module_deps
}

fn find_main_class(bobje: &Bobje) -> Option<String> {
    let java_re = regex!(r"(public\s+)?static\s+void\s+main\s*\(");
    let kotlin_re = regex!(r"fun\s+main\s*\(");
    for source_file in &bobje.source_files {
        if source_file.ends_with(".java")
            && let Ok(contents) = fs::read_to_string(source_file)
            && java_re.is_match(&contents)
        {
            return Some(get_class_name(source_file));
        }
        if source_file.ends_with(".kt")
            && let Ok(contents) = fs::read_to_string(source_file)
            && kotlin_re.is_match(&contents)
        {
            return Some(get_class_name(source_file) + "Kt");
        }
    }
    None
}
