/*
 * Copyright (c) 2023-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

type Job = Box<dyn FnOnce() + Send + 'static>;

// MARK: Options
/// Configuration for a scalable [`ThreadPool`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    /// Minimum number of worker threads kept alive by the pool.
    pub min_worker_threads: usize,
    /// Maximum number of worker threads the pool may create.
    pub max_worker_threads: usize,
    /// Time an excess worker thread may remain idle before it exits.
    pub worker_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        let min_worker_threads = thread::available_parallelism().map_or(1, |count| count.get());
        Self {
            min_worker_threads,
            max_worker_threads: min_worker_threads * 8,
            worker_timeout: Duration::from_secs(30),
        }
    }
}

struct Shared {
    state: Mutex<State>,
    work_available: Condvar,
    options: Options,
}

struct State {
    jobs: VecDeque<Job>,
    worker_threads: usize,
    idle_worker_threads: usize,
    panicked: bool,
    shutdown: bool,
}

struct WorkerThreadGuard {
    shared: Arc<Shared>,
    registered: bool,
}

impl WorkerThreadGuard {
    const fn unregister(&mut self, state: &mut State) {
        state.worker_threads -= 1;
        self.registered = false;
    }
}

impl Drop for WorkerThreadGuard {
    fn drop(&mut self) {
        if self.registered {
            let mut state = self.shared.state.lock().expect("Mutex lock failed");
            state.worker_threads -= 1;
            state.panicked |= thread::panicking();
            self.shared.work_available.notify_all();
        }
    }
}

// MARK: ThreadPool
/// Thread pool for executing tasks on multiple worker threads.
pub struct ThreadPool {
    shared: Arc<Shared>,
    worker_threads: Mutex<Vec<JoinHandle<()>>>,
}

impl ThreadPool {
    /// Creates a scalable thread pool with [`Options::default`].
    pub fn new() -> Self {
        Self::new_with_options(Options::default())
    }

    /// Creates a thread pool with a fixed number of worker threads.
    ///
    /// # Panics
    ///
    /// Panics if `num_worker_threads` is zero.
    pub fn new_fixed(num_worker_threads: usize) -> Self {
        assert!(
            num_worker_threads > 0,
            "Number of worker threads must be greater than 0"
        );
        Self::new_with_options(Options {
            min_worker_threads: num_worker_threads,
            max_worker_threads: num_worker_threads,
            worker_timeout: Duration::from_secs(30),
        })
    }

    /// Creates a scalable thread pool with the supplied options.
    ///
    /// # Panics
    ///
    /// Panics if `min_worker_threads` is zero or `max_worker_threads` is less than
    /// `min_worker_threads`.
    pub fn new_with_options(options: Options) -> Self {
        assert!(
            options.min_worker_threads > 0,
            "Minimum number of worker threads must be greater than 0"
        );
        assert!(
            options.max_worker_threads >= options.min_worker_threads,
            "Maximum number of worker threads must be at least the minimum"
        );

        let min_worker_threads = options.min_worker_threads;
        let pool = Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    jobs: VecDeque::new(),
                    worker_threads: min_worker_threads,
                    idle_worker_threads: 0,
                    panicked: false,
                    shutdown: false,
                }),
                work_available: Condvar::new(),
                options,
            }),
            worker_threads: Mutex::new(Vec::with_capacity(min_worker_threads)),
        };

        for _ in 0..min_worker_threads {
            pool.spawn_worker_thread();
        }
        pool
    }

    /// Executes a closure on an available worker thread.
    pub fn execute(&self, f: impl FnOnce() + Send + 'static) {
        let should_spawn = {
            let mut state = self.shared.state.lock().expect("Mutex lock failed");
            assert!(!state.shutdown, "Thread pool has shut down");
            state.jobs.push_back(Box::new(f));

            let should_spawn = state.jobs.len() > state.idle_worker_threads
                && state.worker_threads < self.shared.options.max_worker_threads;
            if should_spawn {
                state.worker_threads += 1;
            }
            self.shared.work_available.notify_one();
            should_spawn
        };

        if should_spawn {
            self.spawn_worker_thread();
        }
    }

    /// Waits for all worker threads to finish their tasks.
    ///
    /// This consumes the thread pool and joins all worker threads.
    pub fn join(self) {
        self.shutdown();
        let worker_threads = {
            let mut worker_threads = self.worker_threads.lock().expect("Mutex lock failed");
            std::mem::take(&mut *worker_threads)
        };
        for worker_thread in worker_threads {
            worker_thread.join().expect("A worker thread panicked");
        }
        assert!(
            !self
                .shared
                .state
                .lock()
                .expect("Mutex lock failed")
                .panicked,
            "A worker thread panicked"
        );
    }

    fn spawn_worker_thread(&self) {
        let shared = Arc::clone(&self.shared);
        let worker_thread = thread::Builder::new()
            .name("threadpool-worker".to_string())
            .spawn(move || worker_thread_loop(shared))
            .expect("Failed to spawn worker thread");
        let mut worker_threads = self.worker_threads.lock().expect("Mutex lock failed");
        // Release the handles of idle worker threads that exited, a panic is tracked in the state
        worker_threads.retain(|worker_thread| !worker_thread.is_finished());
        worker_threads.push(worker_thread);
    }

    fn shutdown(&self) {
        let mut state = self.shared.state.lock().expect("Mutex lock failed");
        state.shutdown = true;
        self.shared.work_available.notify_all();
    }
}

impl Default for ThreadPool {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_thread_loop(shared: Arc<Shared>) {
    let mut guard = WorkerThreadGuard {
        shared: Arc::clone(&shared),
        registered: true,
    };

    loop {
        let job = {
            let mut state = shared.state.lock().expect("Mutex lock failed");
            loop {
                if let Some(job) = state.jobs.pop_front() {
                    break Some(job);
                }
                if state.shutdown {
                    guard.unregister(&mut state);
                    break None;
                }

                state.idle_worker_threads += 1;
                if state.worker_threads > shared.options.min_worker_threads {
                    let (new_state, result) = shared
                        .work_available
                        .wait_timeout(state, shared.options.worker_timeout)
                        .expect("Mutex lock failed");
                    state = new_state;
                    state.idle_worker_threads -= 1;
                    if result.timed_out()
                        && state.jobs.is_empty()
                        && state.worker_threads > shared.options.min_worker_threads
                    {
                        guard.unregister(&mut state);
                        break None;
                    }
                } else {
                    state = shared
                        .work_available
                        .wait(state)
                        .expect("Mutex lock failed");
                    state.idle_worker_threads -= 1;
                }
            }
        };

        match job {
            Some(job) => job(),
            None => return,
        }
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;

    fn worker_thread_count(pool: &ThreadPool) -> usize {
        pool.shared
            .state
            .lock()
            .expect("Mutex lock failed")
            .worker_threads
    }

    fn wait_for_worker_thread_count(pool: &ThreadPool, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while worker_thread_count(pool) != expected {
            assert!(
                Instant::now() < deadline,
                "Worker thread count did not reach {expected}"
            );
            thread::yield_now();
        }
    }

    fn blocking_job(
        started: mpsc::Sender<()>,
        release: Arc<(Mutex<bool>, Condvar)>,
    ) -> impl FnOnce() + Send + 'static {
        move || {
            started.send(()).expect("Failed to signal task start");
            let (lock, condition) = &*release;
            let mut released = lock.lock().expect("Mutex lock failed");
            while !*released {
                released = condition.wait(released).expect("Mutex lock failed");
            }
        }
    }

    fn release_jobs(release: &(Mutex<bool>, Condvar)) {
        let (lock, condition) = release;
        *lock.lock().expect("Mutex lock failed") = true;
        condition.notify_all();
    }

    #[test]
    fn test_default_options() {
        let options = Options::default();
        let available_worker_threads =
            thread::available_parallelism().map_or(1, |count| count.get());
        assert_eq!(options.min_worker_threads, available_worker_threads);
        assert_eq!(options.max_worker_threads, available_worker_threads * 8);
        assert_eq!(options.worker_timeout, Duration::from_secs(30));
    }

    #[test]
    #[should_panic(expected = "Number of worker threads must be greater than 0")]
    fn test_fixed_pool_zero_worker_threads() {
        let _pool = ThreadPool::new_fixed(0);
    }

    #[test]
    #[should_panic(expected = "Minimum number of worker threads must be greater than 0")]
    fn test_options_zero_minimum() {
        let _pool = ThreadPool::new_with_options(Options {
            min_worker_threads: 0,
            max_worker_threads: 1,
            worker_timeout: Duration::from_secs(1),
        });
    }

    #[test]
    #[should_panic(expected = "Maximum number of worker threads must be at least the minimum")]
    fn test_options_maximum_below_minimum() {
        let _pool = ThreadPool::new_with_options(Options {
            min_worker_threads: 2,
            max_worker_threads: 1,
            worker_timeout: Duration::from_secs(1),
        });
    }

    #[test]
    fn test_execute_single_task() {
        let pool = ThreadPool::new_fixed(1);
        let counter = Arc::new(AtomicUsize::new(0));
        let task_counter = Arc::clone(&counter);
        pool.execute(move || {
            task_counter.fetch_add(1, Ordering::SeqCst);
        });
        pool.join();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_execute_multiple_tasks() {
        let pool = ThreadPool::new_fixed(2);
        let counter = Arc::new(AtomicUsize::new(0));
        let task_count = 10;
        for _ in 0..task_count {
            let task_counter = Arc::clone(&counter);
            pool.execute(move || {
                task_counter.fetch_add(1, Ordering::SeqCst);
            });
        }
        pool.join();
        assert_eq!(counter.load(Ordering::SeqCst), task_count);
    }

    #[test]
    fn test_scalable_pool_grows_and_shrinks() {
        let pool = ThreadPool::new_with_options(Options {
            min_worker_threads: 2,
            max_worker_threads: 6,
            worker_timeout: Duration::from_millis(20),
        });
        let (started_sender, started_receiver) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));

        for _ in 0..6 {
            pool.execute(blocking_job(started_sender.clone(), Arc::clone(&release)));
            started_receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("Task did not start");
        }
        assert_eq!(worker_thread_count(&pool), 6);

        release_jobs(&release);
        wait_for_worker_thread_count(&pool, 2);
        pool.join();
    }

    #[test]
    fn test_exited_worker_threads_are_released() {
        let pool = ThreadPool::new_with_options(Options {
            min_worker_threads: 1,
            max_worker_threads: 3,
            worker_timeout: Duration::from_millis(20),
        });
        let (started_sender, started_receiver) = mpsc::channel();
        for _ in 0..2 {
            let release = Arc::new((Mutex::new(false), Condvar::new()));
            for _ in 0..3 {
                pool.execute(blocking_job(started_sender.clone(), Arc::clone(&release)));
                started_receiver
                    .recv_timeout(Duration::from_secs(1))
                    .expect("Task did not start");
            }
            assert_eq!(
                pool.worker_threads.lock().expect("Mutex lock failed").len(),
                3
            );
            release_jobs(&release);
            wait_for_worker_thread_count(&pool, 1);
            let deadline = Instant::now() + Duration::from_secs(1);
            while pool
                .worker_threads
                .lock()
                .expect("Mutex lock failed")
                .iter()
                .filter(|worker_thread| worker_thread.is_finished())
                .count()
                != 2
            {
                assert!(Instant::now() < deadline, "Worker threads did not exit");
                thread::yield_now();
            }
        }
        pool.join();
    }

    #[test]
    fn test_join_reports_worker_thread_panics() {
        let pool = ThreadPool::new_fixed(1);
        pool.execute(|| panic!("Task failed"));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pool.join()));
        assert!(result.is_err());
    }

    #[test]
    fn test_pool_does_not_exceed_maximum() {
        let pool = ThreadPool::new_with_options(Options {
            min_worker_threads: 1,
            max_worker_threads: 2,
            worker_timeout: Duration::from_secs(1),
        });
        let (started_sender, started_receiver) = mpsc::channel();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        for _ in 0..3 {
            pool.execute(blocking_job(started_sender.clone(), Arc::clone(&release)));
        }

        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("First task did not start");
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Second task did not start");
        assert_eq!(worker_thread_count(&pool), 2);
        assert!(
            started_receiver
                .recv_timeout(Duration::from_millis(20))
                .is_err()
        );

        release_jobs(&release);
        pool.join();
    }

    #[test]
    fn test_fixed_pool_does_not_grow() {
        let pool = ThreadPool::new_fixed(2);
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (started_sender, started_receiver) = mpsc::channel();
        for _ in 0..3 {
            pool.execute(blocking_job(started_sender.clone(), Arc::clone(&release)));
        }

        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("First task did not start");
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("Second task did not start");
        assert_eq!(worker_thread_count(&pool), 2);

        release_jobs(&release);
        pool.join();
    }
}
