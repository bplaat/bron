/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [trybuild](https://crates.io/crates/trybuild) crate

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::panic::RefUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use serde::{Deserialize, Serialize};

/// A collection of compile-pass and compile-fail test cases.
#[derive(Debug)]
pub struct TestCases {
    tests: RefCell<Vec<Test>>,
}

#[derive(Debug)]
struct Test {
    pattern: PathBuf,
    expected: Expected,
}

#[derive(Clone, Copy, Debug)]
enum Expected {
    Pass,
    CompileFail,
}

type Dependencies = BTreeMap<String, serde_json::Value>;

#[derive(Deserialize)]
struct SourceManifest {
    package: SourcePackage,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: Dependencies,
    #[serde(default)]
    target: BTreeMap<String, SourceTarget>,
}

#[derive(Deserialize)]
struct SourceTarget {
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: Dependencies,
}

#[derive(Deserialize)]
struct SourcePackage {
    name: String,
    #[serde(default)]
    edition: Option<Edition>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Edition {
    Value(String),
    Workspace { workspace: bool },
}

#[derive(Deserialize, Default)]
struct WorkspaceManifest {
    #[serde(default)]
    workspace: Workspace,
    #[serde(default)]
    patch: BTreeMap<String, Dependencies>,
}

#[derive(Deserialize, Default)]
struct Workspace {
    #[serde(default)]
    package: WorkspacePackage,
}

#[derive(Deserialize, Default)]
struct WorkspacePackage {
    edition: Option<String>,
}

#[derive(Serialize)]
struct TestManifest {
    package: TestPackage,
    dependencies: Dependencies,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    target: BTreeMap<String, TestTarget>,
    #[serde(rename = "bin")]
    bins: Vec<Bin>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    patch: BTreeMap<String, Dependencies>,
    workspace: Empty,
}

#[derive(Serialize)]
struct TestTarget {
    dependencies: Dependencies,
}

#[derive(Serialize)]
struct TestPackage {
    name: String,
    version: &'static str,
    edition: String,
    publish: bool,
}

#[derive(Serialize)]
struct Bin {
    name: String,
    path: PathBuf,
}

#[derive(Serialize)]
struct Empty {}

#[derive(Deserialize)]
struct CargoMessage {
    reason: String,
    #[serde(default)]
    target: Option<CargoTarget>,
    #[serde(default)]
    message: Option<CompilerMessage>,
    #[serde(default)]
    executable: Option<PathBuf>,
}

#[derive(Deserialize)]
struct CargoTarget {
    name: String,
}

#[derive(Deserialize)]
struct CompilerMessage {
    rendered: Option<String>,
}

impl TestCases {
    /// Creates an empty test collection.
    pub const fn new() -> Self {
        Self {
            tests: RefCell::new(Vec::new()),
        }
    }

    /// Adds files that must compile and run successfully.
    pub fn pass(&self, pattern: impl AsRef<Path>) {
        self.tests.borrow_mut().push(Test {
            pattern: pattern.as_ref().to_owned(),
            expected: Expected::Pass,
        });
    }

    /// Adds files that must fail with their adjacent `.stderr` diagnostics.
    pub fn compile_fail(&self, pattern: impl AsRef<Path>) {
        self.tests.borrow_mut().push(Test {
            pattern: pattern.as_ref().to_owned(),
            expected: Expected::CompileFail,
        });
    }

    fn run(&self) -> Result<(), String> {
        let source_dir = env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| "CARGO_MANIFEST_DIR is not set".to_string())?;
        let source = read_source_manifest(&source_dir)?;
        let edition = resolve_edition(&source_dir, source.package.edition)?;
        let tests = expand_tests(&source_dir, &self.tests.borrow())?;
        let workspace_dir = workspace_root(&source_dir)?;
        let project_dir = workspace_dir
            .join("target/tests/trybuild")
            .join(&source.package.name);

        fs::create_dir_all(&project_dir).map_err(|error| error.to_string())?;
        fs::write(
            project_dir.join("main.rs"),
            "#![allow(missing_docs, unused_crate_dependencies)]\nfn main() {}\n",
        )
        .map_err(|error| error.to_string())?;

        let bins = std::iter::once(Bin {
            name: format!("{}-tests", source.package.name),
            path: PathBuf::from("main.rs"),
        })
        .chain(tests.iter().enumerate().map(|(index, test)| Bin {
            name: format!("trybuild{index:03}"),
            path: test.path.clone(),
        }))
        .collect();
        // Like upstream, the tests can use the crate, its dev-dependencies and the workspace patches
        let mut dependencies = absolute_paths(source.dev_dependencies, &source_dir);
        dependencies.insert(
            source.package.name.clone(),
            serde_json::json!({ "path": source_dir, "default-features": false }),
        );
        let target = source
            .target
            .into_iter()
            .filter(|(_, target)| !target.dev_dependencies.is_empty())
            .map(|(cfg, target)| {
                let dependencies = absolute_paths(target.dev_dependencies, &source_dir);
                (cfg, TestTarget { dependencies })
            })
            .collect();
        let workspace_manifest = fs::read_to_string(workspace_dir.join("Cargo.toml"))
            .map_err(|error| error.to_string())?;
        let patch = basic_toml::from_str::<WorkspaceManifest>(&workspace_manifest)
            .map_err(|error| error.to_string())?
            .patch
            .into_iter()
            .map(|(registry, patches)| (registry, absolute_paths(patches, &workspace_dir)))
            .collect();
        let manifest = TestManifest {
            package: TestPackage {
                name: format!("{}-tests", source.package.name),
                version: "0.0.0",
                edition,
                publish: false,
            },
            dependencies,
            target,
            bins,
            patch,
            workspace: Empty {},
        };
        let manifest = basic_toml::to_string(&manifest).map_err(|error| error.to_string())?;
        fs::write(project_dir.join("Cargo.toml"), manifest).map_err(|error| error.to_string())?;

        let mut failures = 0;
        for (index, test) in tests.iter().enumerate() {
            let name = format!("trybuild{index:03}");
            print!("test {} ... ", test.display);
            match run_test(&project_dir, &source_dir, &name, test) {
                Ok(()) => println!("ok"),
                Err(error) => {
                    failures += 1;
                    println!("FAILED\n{error}");
                }
            }
        }

        if failures == 0 {
            Ok(())
        } else {
            Err(format!("{failures} of {} tests failed", tests.len()))
        }
    }
}

impl Default for TestCases {
    fn default() -> Self {
        Self::new()
    }
}

impl RefUnwindSafe for TestCases {}

impl Drop for TestCases {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && let Err(error) = self.run()
        {
            panic!("{error}");
        }
    }
}

struct ExpandedTest {
    path: PathBuf,
    display: String,
    expected: Expected,
}

fn read_source_manifest(source_dir: &Path) -> Result<SourceManifest, String> {
    let contents =
        fs::read_to_string(source_dir.join("Cargo.toml")).map_err(|error| error.to_string())?;
    basic_toml::from_str(&contents).map_err(|error| error.to_string())
}

fn resolve_edition(source_dir: &Path, edition: Option<Edition>) -> Result<String, String> {
    match edition {
        Some(Edition::Value(edition)) => Ok(edition),
        Some(Edition::Workspace { workspace: true }) | None => {
            for directory in source_dir.ancestors().skip(1) {
                let path = directory.join("Cargo.toml");
                let Ok(contents) = fs::read_to_string(path) else {
                    continue;
                };
                let Ok(manifest) = basic_toml::from_str::<WorkspaceManifest>(&contents) else {
                    continue;
                };
                if let Some(edition) = manifest.workspace.package.edition {
                    return Ok(edition);
                }
            }
            Err("workspace package edition is not defined".to_string())
        }
        Some(Edition::Workspace { workspace: false }) => {
            Err("package.edition.workspace must be true".to_string())
        }
    }
}

fn workspace_root(source_dir: &Path) -> Result<PathBuf, String> {
    source_dir
        .ancestors()
        .find(|directory| {
            fs::read_to_string(directory.join("Cargo.toml"))
                .is_ok_and(|contents| contents.contains("[workspace]"))
        })
        .map(Path::to_path_buf)
        .ok_or_else(|| "workspace root not found".to_string())
}

/// Write dependencies as tables, as TOML needs them after plain values, and resolve relative
/// `path` dependencies against the manifest directory they come from
fn absolute_paths(mut dependencies: Dependencies, base: &Path) -> Dependencies {
    for dependency in dependencies.values_mut() {
        if dependency.is_string() {
            *dependency = serde_json::json!({ "version": dependency });
        }
        if let Some(path) = dependency.get_mut("path")
            && let Some(relative) = path.as_str()
        {
            *path = base.join(relative).to_string_lossy().into();
        }
    }
    dependencies
}

fn expand_tests(source_dir: &Path, tests: &[Test]) -> Result<Vec<ExpandedTest>, String> {
    let mut expanded = Vec::new();
    for test in tests {
        let pattern = source_dir.join(&test.pattern);
        let Some(file_name) = pattern.file_name().and_then(|name| name.to_str()) else {
            return Err(format!("invalid test pattern: {}", test.pattern.display()));
        };
        if !file_name.contains('*') {
            push_test(source_dir, &mut expanded, pattern, test.expected)?;
            continue;
        }

        let parent = pattern
            .parent()
            .ok_or_else(|| "test pattern has no parent".to_string())?;
        let (prefix, suffix) = file_name
            .split_once('*')
            .ok_or_else(|| "test pattern must contain one wildcard".to_string())?;
        let mut paths = fs::read_dir(parent)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(prefix) && name.ends_with(suffix))
            })
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            push_test(source_dir, &mut expanded, path, test.expected)?;
        }
    }
    Ok(expanded)
}

fn push_test(
    source_dir: &Path,
    tests: &mut Vec<ExpandedTest>,
    path: PathBuf,
    expected: Expected,
) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("test file does not exist: {}", path.display()));
    }
    let display = path
        .strip_prefix(source_dir)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/");
    tests.push(ExpandedTest {
        path,
        display,
        expected,
    });
    Ok(())
}

fn run_test(
    project_dir: &Path,
    source_dir: &Path,
    name: &str,
    test: &ExpandedTest,
) -> Result<(), String> {
    let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["build", "--quiet", "--message-format=json", "--bin", name])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .current_dir(project_dir)
        .output()
        .map_err(|error| error.to_string())?;
    let (diagnostics, executable) = parse_cargo_output(&output.stdout, name, source_dir);

    match test.expected {
        Expected::Pass if !output.status.success() => Err(format!(
            "expected successful compilation\n{}{}",
            diagnostics,
            String::from_utf8_lossy(&output.stderr)
        )),
        Expected::Pass => {
            let executable =
                executable.ok_or_else(|| "Cargo did not produce an executable".to_string())?;
            let output = Command::new(executable)
                .output()
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "test executable failed\n{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        }
        Expected::CompileFail if output.status.success() => {
            Err("expected compilation to fail, but it succeeded".to_string())
        }
        Expected::CompileFail => compare_diagnostics(test, &diagnostics),
    }
}

fn parse_cargo_output(stdout: &[u8], name: &str, source_dir: &Path) -> (String, Option<PathBuf>) {
    let mut diagnostics = String::new();
    let mut executable = None;
    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(message) = serde_json::from_str::<CargoMessage>(line) else {
            continue;
        };
        if message
            .target
            .as_ref()
            .is_none_or(|target| target.name != name)
        {
            continue;
        }
        if message.reason == "compiler-message"
            && let Some(rendered) = message.message.and_then(|message| message.rendered)
        {
            diagnostics.push_str(&rendered);
        }
        if message.reason == "compiler-artifact" {
            executable = message.executable;
        }
    }
    (normalize_diagnostics(&diagnostics, source_dir), executable)
}

fn normalize_diagnostics(diagnostics: &str, source_dir: &Path) -> String {
    let source_dir = source_dir.to_string_lossy().replace('\\', "/");
    let mut normalized = diagnostics.replace('\\', "/");
    normalized = normalized.replace(&format!("{source_dir}/"), "");
    let trimmed = normalized.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

fn compare_diagnostics(test: &ExpandedTest, actual: &str) -> Result<(), String> {
    let stderr_path = test.path.with_extension("stderr");
    if env::var("TRYBUILD").as_deref() == Ok("overwrite") {
        fs::write(&stderr_path, actual).map_err(|error| error.to_string())?;
        return Ok(());
    }

    let expected = fs::read_to_string(&stderr_path)
        .map_err(|error| format!("can't read {}: {error}", stderr_path.display()))?
        .replace("\r\n", "\n");
    if expected == actual {
        Ok(())
    } else {
        Err(format!("expected:\n{expected}\nactual:\n{actual}"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::normalize_diagnostics;

    #[test]
    fn normalizes_source_paths_and_trailing_newline() {
        assert_eq!(
            normalize_diagnostics(
                "error: example\n --> /workspace/crate/tests/ui/test.rs:1:1\n\n",
                Path::new("/workspace/crate"),
            ),
            "error: example\n --> tests/ui/test.rs:1:1\n",
        );
    }
}
