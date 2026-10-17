/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unused_variables)]

use std::collections::HashSet;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::{Command, exit};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::{env, fs, thread};

use sha1::Sha1;
use threadpool::ThreadPool;

use crate::artifacts::{add_dex, download_jar, extract_jar, valid_jar};
use crate::log::{Log, LogEntry};

// MARK: Task
#[derive(Debug, Clone)]
pub(crate) struct Task {
    id: usize,
    action: TaskAction,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum TaskAction {
    Phony(String),
    Copy(String, String),
    Download(String, String, String),
    ExtractJar(String, String),
    AddDex(String, String, String),
    Command(String),
    Multiple(Vec<TaskAction>),
}

impl Task {
    fn have_inputs_change(&self, log: &Log) -> bool {
        if let TaskAction::Download(_, destination, _) = &self.action
            && !valid_jar(destination)
        {
            return true;
        }

        // Check if inputs have changed
        for input in &self.inputs {
            // Get input modified time
            let metadata = match fs::metadata(input) {
                Ok(metadata) => metadata,
                Err(_) => return true,
            };
            let mtime = metadata
                .modified()
                .expect("Failed to get modified time")
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("Time went backwards");

            // Get input hash
            let hash = if !metadata.is_dir() {
                // Calculate input hash
                let buffer = fs::read(input).unwrap_or_else(|_| {
                    eprintln!("Can't read input file: {input}");
                    exit(1)
                });
                if !buffer.is_empty() {
                    Some(Sha1::digest(buffer))
                } else {
                    None
                }
            } else {
                None
            };

            // Check if the input has changed
            if log
                .get(input)
                .is_none_or(|entry| entry.mtime != mtime || entry.hash != hash)
            {
                return true;
            }
        }

        // Check if outputs are missing
        for output in &self.outputs {
            if !Path::new(output).exists() {
                return true;
            }
        }

        false
    }

    fn execute(
        &self,
        log: Arc<Mutex<Log>>,
        task_counter: Arc<AtomicUsize>,
        total_tasks: usize,
        pretty_print: bool,
    ) {
        // Update log entries of inputs
        {
            let mut log: std::sync::MutexGuard<'_, Log> = log.lock().expect("Could not lock mutex");
            for input in &self.inputs {
                // Get input modified time
                let metadata = fs::metadata(input).unwrap_or_else(|_| {
                    eprintln!("Can't open input file: {input}");
                    exit(1)
                });
                let mtime = metadata
                    .modified()
                    .expect("Failed to get modified time")
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("Time went backwards");

                // Get input hash
                let hash = if !metadata.is_dir() {
                    // Calculate input hash
                    let buffer = fs::read(input).unwrap_or_else(|_| {
                        eprintln!("Can't read input file: {input}");
                        exit(1)
                    });
                    if !buffer.is_empty() {
                        Some(Sha1::digest(buffer))
                    } else {
                        None
                    }
                } else {
                    None
                };

                if log
                    .get(input)
                    .is_none_or(|entry| entry.mtime != mtime || entry.hash != hash)
                {
                    log.add(LogEntry {
                        path: input.clone(),
                        mtime,
                        hash,
                    });
                }
            }
        }

        // Create output directories
        for output in &self.outputs {
            if let Some(parent) = Path::new(output).parent() {
                fs::create_dir_all(parent).unwrap_or_else(|e| {
                    eprintln!("Can't create output directory: {} {}", parent.display(), e);
                    exit(1)
                });
            }
        }

        // Execute command
        let line = self.action.execute();
        let current_task = task_counter.fetch_add(1, Ordering::SeqCst);
        let line = format!("[{current_task}/{total_tasks}] {line}");
        if pretty_print {
            let term_width = terminal_size::terminal_size()
                .map(|(w, _)| w.0 as usize)
                .unwrap_or(80);
            println!(
                "\x1B[1A\x1B[2K{}",
                if line.len() > term_width {
                    format!("{}...", &line[..term_width - 3])
                } else {
                    line.to_string()
                }
            );
        } else {
            println!("{line}");
        }

        // Update log entries of output dirs
        {
            let mut log = log.lock().expect("Could not lock mutex");
            for output in &self.outputs {
                let metadata = fs::metadata(output).unwrap_or_else(|_| {
                    eprintln!("Can't open output file: {output}");
                    exit(1)
                });
                if metadata.is_dir() {
                    log.add(LogEntry {
                        path: output.clone(),
                        mtime: SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .expect("Time went backwards")
                            - Duration::from_nanos(1),
                        hash: None,
                    });
                }
            }
        }
    }
}

impl TaskAction {
    fn execute(&self) -> String {
        match self {
            TaskAction::Phony(dest) => dest.clone(),
            TaskAction::Copy(src, dst) => {
                fs::copy(src, dst).unwrap_or_else(|_| {
                    eprintln!("Failed to copy {src} to {dst}");
                    exit(1)
                });
                format!("cp {src} {dst}")
            }
            TaskAction::Download(url, destination, lock_file) => {
                download_jar(url, destination, lock_file).unwrap_or_else(|error| {
                    eprintln!("Failed to download {url}: {error}");
                    exit(1);
                });
                format!("download {url} to {destination}")
            }
            TaskAction::ExtractJar(source, destination) => {
                extract_jar(source, destination).unwrap_or_else(|error| {
                    eprintln!("Failed to extract {source}: {error}");
                    exit(1);
                });
                format!("extract classes from {source} to {destination}")
            }
            TaskAction::AddDex(source, dex, destination) => {
                add_dex(source, dex, destination).unwrap_or_else(|error| {
                    eprintln!("Failed to add {dex} to {source}: {error}");
                    exit(1);
                });
                format!("add {dex} to {destination}")
            }
            TaskAction::Command(command) => {
                let status = if cfg!(windows) {
                    let parts = command.split(' ').collect::<Vec<_>>();
                    Command::new(parts[0]).args(&parts[1..]).status()
                } else {
                    Command::new("sh").arg("-c").arg(command).status()
                }
                .unwrap_or_else(|_| {
                    eprintln!("Failed to execute command: {command}");
                    exit(1)
                });
                if !status.success() {
                    eprintln!("Command failed: {command}");
                    exit(1);
                }
                command.clone()
            }
            TaskAction::Multiple(actions) => {
                let mut lines = Vec::new();
                for action in actions {
                    lines.push(action.execute());
                }
                lines.join(" && ")
            }
        }
    }
}

// MARK: ExecutorBuilder
pub(crate) struct ExecutorBuilder {
    tasks_id_counter: usize,
    tasks: Vec<Task>,
}

impl ExecutorBuilder {
    pub(crate) const fn new() -> Self {
        Self {
            tasks_id_counter: 0,
            tasks: Vec::new(),
        }
    }

    pub(crate) fn add_task(
        &mut self,
        action: TaskAction,
        inputs: Vec<String>,
        outputs: Vec<String>,
    ) {
        if !self.tasks.iter().any(|task| task.outputs == outputs) {
            self.tasks.push(Task {
                id: self.tasks_id_counter,
                action,
                inputs,
                outputs,
            });
            self.tasks_id_counter += 1;
        }
    }

    pub(crate) fn add_task_phony(&mut self, inputs: Vec<String>, outputs: Vec<String>) {
        self.add_task(TaskAction::Phony(outputs.join(" ")), inputs, outputs);
    }

    pub(crate) fn add_task_cmd(
        &mut self,
        command: String,
        inputs: Vec<String>,
        outputs: Vec<String>,
    ) {
        self.add_task(TaskAction::Command(command), inputs, outputs);
    }

    pub(crate) fn add_task_cp(&mut self, src: String, dst: String) {
        self.add_task(
            TaskAction::Copy(src.clone(), dst.clone()),
            vec![src],
            vec![dst],
        );
    }

    pub(crate) fn build(self, log_path: &str) -> Executor {
        Executor::new(self.tasks, log_path)
    }
}

// MARK: Circular Dependency Detection
fn detect_circular_dependencies(tasks: &[Task]) {
    for task in tasks {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        if has_cycle(task, tasks, &mut visited, &mut rec_stack) {
            // Build the cycle path for error reporting
            let mut cycle_path = Vec::new();
            let mut current_stack = HashSet::new();
            find_cycle_path(task, tasks, &mut current_stack, &mut cycle_path);

            eprintln!("Circular dependency in tasks detected!\n");
            eprintln!("Problematic tasks:");
            for task_id in &cycle_path {
                for t in tasks {
                    if t.id == *task_id {
                        eprintln!(
                            "  Task {}: inputs={:?}, outputs={:?}",
                            t.id, t.inputs, t.outputs
                        );
                    }
                }
            }
            eprintln!("\nDependency cycle:");
            for (i, task_id) in cycle_path.iter().enumerate() {
                for t in tasks {
                    if t.id == *task_id {
                        eprint!("  Task {} (outputs: {:?})", t.id, t.outputs);
                        if i < cycle_path.len() - 1 {
                            eprintln!(" →");
                        } else {
                            eprintln!(" → [CYCLE]");
                        }
                    }
                }
            }
            exit(1);
        }
    }
}

fn has_cycle(
    task: &Task,
    all_tasks: &[Task],
    visited: &mut HashSet<usize>,
    rec_stack: &mut HashSet<usize>,
) -> bool {
    visited.insert(task.id);
    rec_stack.insert(task.id);

    for input in &task.inputs {
        for other_task in all_tasks {
            if other_task.outputs.contains(input) {
                if !visited.contains(&other_task.id) {
                    if has_cycle(other_task, all_tasks, visited, rec_stack) {
                        return true;
                    }
                } else if rec_stack.contains(&other_task.id) {
                    return true;
                }
            }
        }
    }

    rec_stack.remove(&task.id);
    false
}

fn find_cycle_path(
    task: &Task,
    all_tasks: &[Task],
    current_stack: &mut HashSet<usize>,
    cycle_path: &mut Vec<usize>,
) -> bool {
    current_stack.insert(task.id);
    cycle_path.push(task.id);

    for input in &task.inputs {
        for other_task in all_tasks {
            if other_task.outputs.contains(input) {
                if current_stack.contains(&other_task.id) {
                    // Found the cycle, keep only the cycle part
                    while let Some(&id) = cycle_path.first() {
                        if id == other_task.id {
                            break;
                        }
                        cycle_path.remove(0);
                    }
                    cycle_path.push(other_task.id);
                    return true;
                }
                if find_cycle_path(other_task, all_tasks, current_stack, cycle_path) {
                    return true;
                }
            }
        }
    }

    cycle_path.pop();
    current_stack.remove(&task.id);
    false
}

// MARK: Executor
pub(crate) struct Executor {
    log: Arc<Mutex<Log>>,
    tasks: Vec<Task>,
}

impl Executor {
    fn new(tasks: Vec<Task>, log_path: &str) -> Self {
        // Detect circular dependencies before processing
        detect_circular_dependencies(&tasks);

        fn visit_task(
            task: &Task,
            all_tasks: &[Task],
            new_tasks: &mut Vec<Task>,
            log: &Log,
        ) -> bool {
            if new_tasks.iter().any(|t| t.id == task.id) {
                return false;
            }

            let mut inputs_changed = task.have_inputs_change(log);
            for input in &task.inputs {
                for other_task in all_tasks {
                    if other_task.outputs.contains(input) {
                        inputs_changed |= visit_task(other_task, all_tasks, new_tasks, log);
                    }
                }
            }

            if inputs_changed {
                new_tasks.push(task.clone());
            }
            inputs_changed
        }

        // Create new task tree with all needed tasks
        let log = Log::new(log_path);
        let mut new_tasks = Vec::new();
        let last_task = tasks.last().expect("No tasks to execute");
        visit_task(last_task, &tasks, &mut new_tasks, &log);

        Self {
            log: Arc::new(Mutex::new(log)),
            tasks: new_tasks,
        }
    }

    pub(crate) const fn total_tasks(&self) -> usize {
        self.tasks.len()
    }

    pub(crate) fn execute(&mut self, verbose: bool, thread_count: Option<usize>) {
        // Start execution if there is a last task
        if let Some(last_task) = self.tasks.last() {
            // Print task tree
            if verbose {
                println!("{:#?}", self.tasks);
            }

            let pretty_print = !verbose
                && io::stdout().is_terminal()
                && env::var_os("NO_COLOR").is_none()
                && env::var_os("CI").is_none();
            if pretty_print {
                println!();
            }

            let pool = ThreadPool::new_fixed(
                thread_count
                    .unwrap_or_else(|| thread::available_parallelism().map_or(1, |n| n.get())),
            );
            self.execute_task(
                last_task,
                &pool,
                Arc::new(Mutex::new(Vec::new())),
                Arc::new(Mutex::new(Vec::new())),
                Arc::new(AtomicUsize::new(1)),
                pretty_print,
            );
            pool.join();
        }
    }

    fn execute_task(
        &self,
        task: &Task,
        pool: &ThreadPool,
        scheduled_task_ids: Arc<Mutex<Vec<usize>>>,
        done_task_ids: Arc<Mutex<Vec<usize>>>,
        task_counter: Arc<AtomicUsize>,
        pretty_print: bool,
    ) {
        // Check if task is already scheduled
        {
            let mut scheduled_task_ids = scheduled_task_ids.lock().expect("Could not lock mutex");
            if scheduled_task_ids.contains(&task.id) {
                return;
            }
            scheduled_task_ids.push(task.id);
        }

        // Find and run dependencies
        let mut dependency_ids = Vec::new();
        for input in &task.inputs {
            for other_task in &self.tasks {
                if other_task.outputs.contains(input) {
                    dependency_ids.push(other_task.id);
                    self.execute_task(
                        other_task,
                        pool,
                        scheduled_task_ids.clone(),
                        done_task_ids.clone(),
                        task_counter.clone(),
                        pretty_print,
                    );
                }
            }
        }

        let task = task.clone();
        let log = self.log.clone();
        let task_counter: Arc<AtomicUsize> = task_counter.clone();
        let total_tasks = self.tasks.len();
        pool.execute(move || {
            // Wait for dependencies to finish
            if !dependency_ids.is_empty() {
                while {
                    let done_task_ids = done_task_ids.lock().expect("Could not lock mutex");
                    !dependency_ids
                        .iter()
                        .all(|dep_id| done_task_ids.contains(dep_id))
                } {
                    thread::yield_now();
                }
            }

            // Execute task
            task.execute(log, task_counter, total_tasks, pretty_print);

            // Mark task as done
            {
                let mut done_ids = done_task_ids.lock().expect("Could not lock mutex");
                done_ids.push(task.id);
            }
        });
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_reuses_existing_file_while_holding_lock() {
        let directory = env::temp_dir().join(format!("bob-download-test-{}", std::process::id()));
        _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("create test directory");
        let destination = directory.join("dependency.jar");
        let lock_file = directory.join("download.lock");
        let mut jar = b"PK\x05\x06".to_vec();
        jar.extend_from_slice(&[0; 18]);
        fs::write(&destination, &jar).expect("write existing download");

        let action = TaskAction::Download(
            "invalid://unused".to_owned(),
            destination.to_string_lossy().into_owned(),
            lock_file.to_string_lossy().into_owned(),
        );
        action.execute();

        assert_eq!(fs::read(destination).expect("read existing download"), jar);
        assert!(lock_file.exists());
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
