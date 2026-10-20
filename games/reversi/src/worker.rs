/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use bwindow::EventLoopProxy;
use reversi_engine::{Move, Othello, Player};

use crate::AppEvent;

#[derive(Clone, Copy)]
pub(crate) struct AiJob {
    pub(crate) revision: u64,
    pub(crate) board: Othello,
}

pub(crate) struct AiResult {
    pub(crate) job: AiJob,
    pub(crate) movement: Option<Move>,
}

/// State shared between the UI thread and the worker thread.
#[derive(Default)]
struct Shared {
    revision: AtomicU64,
    stop: AtomicBool,
}

impl Shared {
    fn is_cancelled(&self, job: &AiJob) -> bool {
        self.stop.load(Ordering::Relaxed) || self.revision.load(Ordering::Relaxed) != job.revision
    }
}

pub(crate) struct AiWorker {
    sender: Option<mpsc::Sender<AiJob>>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl AiWorker {
    pub(crate) fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        let (sender, receiver) = mpsc::channel::<AiJob>();
        let shared = Arc::new(Shared::default());
        let thread = thread::Builder::new()
            .name("ai-worker".into())
            .spawn({
                let shared = shared.clone();
                move || {
                    while let Ok(job) = receiver.recv() {
                        // Only search the newest queued position after a reset.
                        let job = receiver.try_iter().last().unwrap_or(job);
                        if shared.stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let cancelled = || shared.is_cancelled(&job);
                        if cancelled() {
                            continue;
                        }
                        let movement = job.board.compute_move_cancellable(
                            Player::White,
                            64,
                            500_000,
                            &cancelled,
                        );
                        if !cancelled()
                            && proxy
                                .send_user_event(AppEvent::Ai(AiResult { job, movement }))
                                .is_err()
                        {
                            break;
                        }
                    }
                }
            })
            .expect("Can't spawn AI worker thread");
        Self {
            sender: Some(sender),
            shared,
            thread: Some(thread),
        }
    }

    pub(crate) fn submit(&self, job: AiJob) {
        self.cancel(job.revision);
        self.sender
            .as_ref()
            .expect("AI worker running")
            .send(job)
            .expect("AI worker stopped");
    }

    /// Cancel every search that is not for the given revision.
    pub(crate) fn cancel(&self, revision: u64) {
        self.shared.revision.store(revision, Ordering::Relaxed);
    }
}

impl Drop for AiWorker {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        // Closing the channel wakes the worker so it can exit
        drop(self.sender.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
