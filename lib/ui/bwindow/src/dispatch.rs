/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use crate::NativeEvent;

type Handler<E> = Box<dyn FnMut(E)>;

struct Pending<E> {
    event: E,
    before: Option<Box<dyn FnOnce()>>,
    after: Option<Box<dyn FnOnce()>>,
}

struct Dispatcher<E> {
    handler: RefCell<Option<Handler<E>>>,
    pending: RefCell<VecDeque<Pending<E>>>,
    dispatching: Cell<bool>,
    closed: Cell<bool>,
}

impl<E> Dispatcher<E> {
    const fn new() -> Self {
        Self {
            handler: RefCell::new(None),
            pending: RefCell::new(VecDeque::new()),
            dispatching: Cell::new(false),
            closed: Cell::new(false),
        }
    }

    fn start(&self, handler: impl FnMut(E) + 'static) {
        assert!(!self.closed.get(), "event loop cannot be restarted");
        assert!(
            self.handler.borrow().is_none(),
            "event loop already running"
        );
        *self.handler.borrow_mut() = Some(Box::new(handler));
        self.flush();
    }

    fn send(&self, event: E) {
        self.enqueue(event, None, None);
    }

    fn send_before(&self, event: E, before: impl FnOnce() + 'static) {
        self.enqueue(event, Some(Box::new(before)), None);
    }

    fn send_then(&self, event: E, after: impl FnOnce() + 'static) {
        self.enqueue(event, None, Some(Box::new(after)));
    }

    fn enqueue(
        &self,
        event: E,
        before: Option<Box<dyn FnOnce()>>,
        after: Option<Box<dyn FnOnce()>>,
    ) {
        if self.closed.get() {
            return;
        }
        self.pending.borrow_mut().push_back(Pending {
            event,
            before,
            after,
        });
        self.flush();
    }

    fn flush(&self) {
        if self.dispatching.get() || self.handler.borrow().is_none() {
            return;
        }
        self.dispatching.set(true);
        while !self.closed.get() {
            let pending = self.pending.borrow_mut().pop_front();
            let Some(mut pending) = pending else { break };
            if let Some(before) = pending.before.take() {
                before();
            }
            // No queue or handler borrow is held while application/native code runs.
            let mut handler = self
                .handler
                .borrow_mut()
                .take()
                .expect("missing event handler");
            handler(pending.event);
            if !self.closed.get() {
                *self.handler.borrow_mut() = Some(handler);
                if let Some(after) = pending.after {
                    after();
                }
            }
        }
        self.dispatching.set(false);
    }

    fn try_paint(&self, event: E, complete: impl FnOnce()) -> bool {
        if self.closed.get() || self.dispatching.get() || self.handler.borrow().is_none() {
            return false;
        }
        self.dispatching.set(true);
        let mut handler = self
            .handler
            .borrow_mut()
            .take()
            .expect("missing event handler");
        handler(event);
        // End the native paint scope before delivering queued application events.
        complete();
        if !self.closed.get() {
            *self.handler.borrow_mut() = Some(handler);
        }
        self.dispatching.set(false);
        self.flush();
        true
    }

    fn stop(&self) {
        self.closed.set(true);
        let handler = self.handler.borrow_mut().take();
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        // Destructors may send events or call stop again. Hold no RefCell borrows.
        drop(handler);
        drop(pending);
    }

    fn shutdown(&self, mailbox: &crate::mailbox::Mailbox, final_event: Option<E>) {
        let pending = mailbox.close();
        self.closed.set(true);
        let handler = self.handler.borrow_mut().take();
        if let (Some(mut handler), Some(event)) = (handler, final_event) {
            handler(event);
        }
        self.stop();
        drop(pending);
    }
}

thread_local! {
    static DISPATCHER: Dispatcher<NativeEvent> = const { Dispatcher::new() };
}

pub(crate) fn start(handler: impl FnMut(NativeEvent) + 'static) {
    DISPATCHER.with(|dispatcher| dispatcher.start(handler));
}

// Disable both delivery paths before dropping payloads, then let the backend
// release its native wake resources. Also used when a loop is dropped before run.
pub(crate) fn shutdown(mailbox: &crate::mailbox::Mailbox) {
    DISPATCHER.with(|dispatcher| dispatcher.shutdown(mailbox, Some(crate::Event::LoopExiting)));
}

pub(crate) fn send(event: NativeEvent) {
    DISPATCHER.with(|dispatcher| dispatcher.send(event));
}

pub(crate) fn send_before(event: NativeEvent, before: impl FnOnce() + 'static) {
    DISPATCHER.with(|dispatcher| dispatcher.send_before(event, before));
}

// A borrowed native frame cannot be queued. Finish it before draining nested events.
pub(crate) fn try_paint(event: NativeEvent, complete: impl FnOnce()) -> bool {
    DISPATCHER.with(|dispatcher| dispatcher.try_paint(event, complete))
}

// Close decisions run after the application has had a chance to cancel them.
pub(crate) fn send_then(event: NativeEvent, after: impl FnOnce() + 'static) {
    DISPATCHER.with(|dispatcher| dispatcher.send_then(event, after));
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::Dispatcher;
    use crate::CloseRequest;

    struct OnDrop(Box<dyn Fn()>);

    impl Drop for OnDrop {
        fn drop(&mut self) {
            (self.0)();
        }
    }

    #[test]
    fn stopping_inside_a_callback_does_not_restore_the_handler() {
        let dispatcher = Rc::new(Dispatcher::new());
        let dropped = Rc::new(Cell::new(false));
        let output = dropped.clone();
        let guard = OnDrop(Box::new(move || output.set(true)));
        let nested = dispatcher.clone();
        dispatcher.start(move |event| {
            let _keep_alive = &guard;
            assert_eq!(event, 1);
            nested.send_then(2, || panic!("queued completion after stop"));
            nested.stop();
            nested.send(3);
        });
        dispatcher.send_then(1, || panic!("close completion after stop"));
        assert!(dropped.get());
        assert!(dispatcher.handler.borrow().is_none());
        assert!(dispatcher.pending.borrow().is_empty());
    }

    #[test]
    fn stopping_during_paint_finishes_the_frame_before_dropping_the_handler() {
        let dispatcher = Rc::new(Dispatcher::new());
        let finished = Rc::new(Cell::new(false));
        let output = finished.clone();
        let guard = OnDrop(Box::new(move || {
            assert!(output.get(), "frame still active")
        }));
        let nested = dispatcher.clone();
        dispatcher.start(move |()| {
            let _keep_alive = &guard;
            nested.stop();
        });
        assert!(dispatcher.try_paint((), || finished.set(true)));
        assert!(finished.get());
        assert!(dispatcher.handler.borrow().is_none());
        assert!(!dispatcher.try_paint((), || panic!("paint after stop")));
    }

    #[test]
    fn pending_payload_destructors_can_reenter_stop_and_send() {
        let dispatcher = Rc::new(Dispatcher::new());
        let dropped = Rc::new(Cell::new(false));
        let output = dropped.clone();
        let nested = Rc::downgrade(&dispatcher);
        dispatcher.send(Some(OnDrop(Box::new(move || {
            let dispatcher = nested.upgrade().expect("dispatcher alive");
            dispatcher.stop();
            dispatcher.send(None);
            output.set(true);
        }))));
        dispatcher.stop();
        assert!(dropped.get());
        assert!(dispatcher.pending.borrow().is_empty());
    }

    #[test]
    fn shutdown_rejects_messages_before_dropping_the_handler() {
        let dispatcher = Rc::new(Dispatcher::new());
        let mailbox = std::sync::Arc::new(crate::mailbox::Mailbox::new());
        let sender = mailbox.clone();
        let dropped = Rc::new(Cell::new(false));
        let output = dropped.clone();
        let nested = Rc::downgrade(&dispatcher);
        let guard = OnDrop(Box::new(move || {
            assert!(!sender.send(crate::mailbox::Message::Exit, || panic!(
                "wake after shutdown"
            )));
            nested.upgrade().expect("dispatcher alive").send(());
            output.set(true);
        }));
        dispatcher.start(move |()| {
            let _keep_alive = &guard;
            panic!("event delivered during shutdown");
        });
        dispatcher.shutdown(&mailbox, None);
        dispatcher.shutdown(&mailbox, None);
        assert!(dropped.get());
        assert!(dispatcher.handler.borrow().is_none());
    }

    #[test]
    fn shutdown_delivers_one_final_event_after_closing_both_event_sources() {
        let dispatcher = Rc::new(Dispatcher::new());
        let mailbox = std::sync::Arc::new(crate::mailbox::Mailbox::new());
        let sender = mailbox.clone();
        let nested = dispatcher.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        dispatcher.start(move |event| {
            output.borrow_mut().push(event);
            assert!(!sender.send(crate::mailbox::Message::Exit, || panic!(
                "wake during shutdown"
            )));
            nested.send(2);
        });
        dispatcher.shutdown(&mailbox, Some(1));
        dispatcher.shutdown(&mailbox, Some(1));
        assert_eq!(*seen.borrow(), [1]);
        assert!(dispatcher.handler.borrow().is_none());
        assert!(dispatcher.pending.borrow().is_empty());
    }

    #[test]
    fn paint_scope_ends_before_queued_events_and_reentrant_paint_is_rejected() {
        let dispatcher = Rc::new(Dispatcher::new());
        assert!(!dispatcher.try_paint(0, || panic!("not running")));
        let painting = Rc::new(Cell::new(true));
        let nested = dispatcher.clone();
        let active = painting.clone();
        dispatcher.start(move |event| {
            if event == 1 {
                assert!(active.get());
                assert!(!nested.try_paint(3, || panic!("reentrant paint")));
                nested.send(2);
            } else {
                assert_eq!(event, 2);
                assert!(!active.get());
            }
        });
        assert!(dispatcher.try_paint(1, || painting.set(false)));
        dispatcher.stop();
        assert_eq!(Rc::strong_count(&dispatcher), 1);
    }

    #[test]
    fn startup_and_nested_events_keep_fifo_order() {
        let dispatcher = Rc::new(Dispatcher::new());
        let seen = Rc::new(RefCell::new(Vec::new()));
        dispatcher.send(1);
        dispatcher.send(2);
        let nested = dispatcher.clone();
        let output = seen.clone();
        dispatcher.start(move |event| {
            // A recursive callback would fail this mutable borrow.
            let mut output = output.borrow_mut();
            output.push(event);
            if event == 1 {
                nested.send(3);
            }
        });
        assert_eq!(*seen.borrow(), [1, 2, 3]);
        dispatcher.stop();
        assert_eq!(Rc::strong_count(&dispatcher), 1);
    }

    #[test]
    fn queued_before_callback_runs_immediately_before_delivery() {
        let dispatcher = Rc::new(Dispatcher::new());
        let state = Rc::new(Cell::new(0));
        let before = state.clone();
        dispatcher.send_before(7, move || before.set(1));
        assert_eq!(state.get(), 0);

        let observed = state.clone();
        dispatcher.start(move |event| {
            assert_eq!(event, 7);
            assert_eq!(observed.get(), 1);
            observed.set(2);
        });
        assert_eq!(state.get(), 2);
        dispatcher.stop();
    }

    #[test]
    fn nested_close_completes_after_cancellation() {
        let dispatcher = Rc::new(Dispatcher::new());
        let closed = Rc::new(RefCell::new(None));
        let nested = dispatcher.clone();
        let result = closed.clone();
        let output = closed.clone();
        dispatcher.start(move |event: Option<CloseRequest>| {
            if let Some(request) = event {
                request.prevent_default();
            } else {
                let request = CloseRequest::new();
                let completion = request.clone();
                let output = output.clone();
                nested.send_then(Some(request), move || {
                    *output.borrow_mut() = Some(!completion.default_prevented());
                });
                assert!(closed.borrow().is_none());
            }
        });
        dispatcher.send(None);
        assert_eq!(*result.borrow(), Some(false));
        dispatcher.stop();
        assert_eq!(Rc::strong_count(&dispatcher), 1);
    }
}
