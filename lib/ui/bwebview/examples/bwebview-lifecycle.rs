/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Desktop smoke check: closing the first window must not stop proxy delivery.

use std::cell::Cell;
use std::rc::Rc;

use bwebview::WebviewBuilder;
use bwindow::{Event, EventLoopBuilder, WindowBuilder, WindowEvent};

enum AppEvent {
    CloseFirst,
    CloseLast,
}

struct ShutdownCheck {
    delivered: Rc<Cell<bool>>,
    proxy: bwindow::EventLoopProxy<AppEvent>,
}

impl Drop for ShutdownCheck {
    fn drop(&mut self) {
        assert!(self.delivered.get(), "last window never received its event");
        assert!(self.proxy.send_user_event(AppEvent::CloseLast).is_err());
    }
}

fn main() {
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .build();
    let proxy = event_loop.create_proxy();
    let late_proxy = event_loop.create_proxy();
    let mut first = WindowBuilder::new().title("Lifecycle: first").build();
    let mut last = WindowBuilder::new().title("Lifecycle: last").build();
    let mut first_content = Some(
        WebviewBuilder::new(&first)
            .load_html("First window")
            .build(),
    );
    let mut last_content = WebviewBuilder::new(&last).load_html("Last window").build();
    let delivered = Rc::new(Cell::new(false));
    let shutdown = ShutdownCheck {
        delivered: delivered.clone(),
        proxy: late_proxy,
    };
    let mut created = 0;
    event_loop.run(move |event| {
        let _keep_alive = &shutdown;
        match event {
            Event::Window(_, WindowEvent::Create) => {
                created += 1;
                if created == 2 {
                    proxy
                        .send_user_event(AppEvent::CloseFirst)
                        .expect("loop closed early");
                }
            }
            Event::UserEvent(AppEvent::CloseFirst) => {
                drop(first_content.take());
                first.close();
                proxy
                    .send_user_event(AppEvent::CloseLast)
                    .expect("first close stopped loop");
            }
            Event::UserEvent(AppEvent::CloseLast) => {
                delivered.set(true);
                last.close();
                last_content.evaluate_script("document.title = 'closed'");
                assert!(last_content.url().is_none());
            }
            _ => {}
        }
    });
}
