/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::any::Any;
use std::collections::VecDeque;
use std::sync::Mutex;

pub(crate) enum Message {
    User(Box<dyn Any + Send>),
    Exit,
    #[cfg(windows)]
    Activate,
}

pub(crate) struct Mailbox(Mutex<Option<VecDeque<Message>>>);

impl Mailbox {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(Some(VecDeque::new())))
    }

    // Waking must only schedule work, never synchronously dispatch it.
    pub(crate) fn send(&self, message: Message, wake: impl FnOnce() -> bool) -> bool {
        let mut state = self.0.lock().expect("event mailbox poisoned");
        let Some(queue) = state.as_mut() else {
            return false;
        };
        queue.push_back(message);
        if wake() {
            true
        } else {
            let failed = queue.pop_back();
            drop(state);
            drop(failed);
            false
        }
    }

    pub(crate) fn pop(&self) -> Option<Message> {
        self.0
            .lock()
            .expect("event mailbox poisoned")
            .as_mut()?
            .pop_front()
    }

    // Reject new messages and return pending payloads for destruction after dispatch stops.
    pub(crate) fn close(&self) -> Option<VecDeque<Message>> {
        self.0.lock().expect("event mailbox poisoned").take()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{Mailbox, Message};

    struct Payload(Arc<AtomicUsize>);

    impl Drop for Payload {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn shutdown_drops_pending_payloads_and_rejects_late_messages() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mailbox = Arc::new(Mailbox::new());
        let worker = mailbox.clone();
        let payload = Payload(dropped.clone());
        std::thread::spawn(move || {
            assert!(worker.send(Message::User(Box::new(payload)), || true));
        })
        .join()
        .expect("worker panicked");
        drop(mailbox.close());
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert!(!mailbox.send(Message::Exit, || panic!("closed mailbox must not wake")));
        assert!(mailbox.pop().is_none());
    }

    #[test]
    fn failed_wake_discards_only_the_failed_message() {
        let mailbox = Mailbox::new();
        assert!(mailbox.send(Message::User(Box::new(42u32)), || true));
        assert!(!mailbox.send(Message::Exit, || false));
        let Some(Message::User(value)) = mailbox.pop() else {
            panic!("missing user event")
        };
        assert_eq!(*value.downcast::<u32>().expect("wrong payload"), 42);
        assert!(mailbox.pop().is_none());
    }

    #[test]
    fn payload_destructors_can_send_after_close_and_failed_wake() {
        struct ReentrantPayload {
            mailbox: Arc<Mailbox>,
            dropped: Arc<AtomicUsize>,
            expect_open: bool,
        }

        impl Drop for ReentrantPayload {
            fn drop(&mut self) {
                assert_eq!(self.mailbox.send(Message::Exit, || true), self.expect_open);
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }

        for expect_open in [false, true] {
            let mailbox = Arc::new(Mailbox::new());
            let dropped = Arc::new(AtomicUsize::new(0));
            let payload = ReentrantPayload {
                mailbox: mailbox.clone(),
                dropped: dropped.clone(),
                expect_open,
            };
            assert_eq!(
                mailbox.send(Message::User(Box::new(payload)), || !expect_open),
                !expect_open
            );
            if expect_open {
                assert!(matches!(mailbox.pop(), Some(Message::Exit)));
            } else {
                drop(mailbox.close());
            }
            assert_eq!(dropped.load(Ordering::Relaxed), 1);
            assert!(mailbox.pop().is_none());
        }
    }
}
