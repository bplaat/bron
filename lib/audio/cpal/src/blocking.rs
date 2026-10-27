/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Stream for blocking backends, a thread opens the device and transfers buffers while the stream
//! is playing.

use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};

use crate::{Error, ErrorCallback, ErrorKind};

/// A blocking device stream, it is owned by the stream thread and closed when dropped.
pub(crate) trait Transfer {
    /// Starts or resumes the device.
    fn start(&mut self) -> Result<(), Error>;

    /// Stops the device.
    fn stop(&mut self) -> Result<(), Error>;

    /// Waits a short while for the device and transfers a buffer with the data callback.
    fn transfer(&mut self) -> Result<(), Error>;
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    Paused,
    Playing,
    Closed,
}

type Shared = Arc<(Mutex<State>, Condvar)>;

pub(crate) struct Stream {
    shared: Shared,
    thread: Option<JoinHandle<()>>,
}

impl Stream {
    /// Opens the device on a new stream thread, so thread bound APIs like COM stay on that thread.
    pub(crate) fn spawn<T: Transfer>(
        open: impl FnOnce() -> Result<T, Error> + Send + 'static,
        error: ErrorCallback,
    ) -> Result<Self, Error> {
        let shared = Arc::new((Mutex::new(State::Paused), Condvar::new()));
        let (opened_sender, opened_receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("cpal-stream".to_string())
            .spawn({
                let shared = shared.clone();
                move || match open() {
                    Ok(device) => {
                        _ = opened_sender.send(Ok(()));
                        stream_thread(&shared, device, error);
                    }
                    Err(err) => _ = opened_sender.send(Err(err)),
                }
            })
            .map_err(|err| Error::with_message(ErrorKind::BackendError, err.to_string()))?;
        let opened = opened_receiver
            .recv()
            .unwrap_or_else(|_| Err(Error::new(ErrorKind::BackendError)));
        let stream = Self {
            shared,
            thread: Some(thread),
        };
        opened.map(|()| stream)
    }

    fn set_state(&self, state: State) {
        let (lock, condvar) = &*self.shared;
        *lock.lock().expect("Failed to lock stream state") = state;
        condvar.notify_all();
    }

    pub(crate) fn play(&self) -> Result<(), Error> {
        self.set_state(State::Playing);
        Ok(())
    }

    pub(crate) fn pause(&self) -> Result<(), Error> {
        self.set_state(State::Paused);
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.set_state(State::Closed);
        if let Some(thread) = self.thread.take() {
            _ = thread.join();
        }
    }
}

fn stream_thread(shared: &Shared, mut device: impl Transfer, mut error: ErrorCallback) {
    let (lock, condvar) = &**shared;
    let mut running = false;
    loop {
        let state = *lock.lock().expect("Failed to lock stream state");
        let result = match (state, running) {
            (State::Closed, _) => break,
            (State::Playing, false) => device.start().map(|()| running = true),
            (State::Paused, true) => device.stop().map(|()| running = false),
            (State::Playing, true) => device.transfer(),
            (State::Paused, false) => {
                drop(
                    condvar
                        .wait_while(lock.lock().expect("Failed to lock stream state"), |state| {
                            *state == State::Paused
                        })
                        .expect("Failed to lock stream state"),
                );
                Ok(())
            }
        };
        if let Err(err) = result {
            error(err);
            break;
        }
    }
}
