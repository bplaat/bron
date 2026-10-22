/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::rc::Rc;

use super::webview::{PlatformWebview as NativeWebview, WebviewInit};
use crate::{InjectionTime, WebviewBuilder, WebviewInterface};

type Operation = Box<dyn FnOnce(&mut NativeWebview)>;

struct State {
    native: NativeWebview,
    ready: bool,
    pending: Vec<Operation>,
}

pub(crate) struct PlatformWebview(Rc<RefCell<State>>);

impl PlatformWebview {
    pub(crate) fn new(attachment: bwindow::WindowAttachment) -> Self {
        Self(Rc::new(RefCell::new(State {
            native: NativeWebview::new(attachment),
            ready: false,
            pending: Vec::new(),
        })))
    }

    pub(crate) fn init_webview(&mut self, builder: WebviewBuilder<'_>) {
        let init = WebviewInit::from(builder);
        let weak = Rc::downgrade(&self.0);
        let initialize = move || {
            let Some(state) = weak.upgrade() else { return };
            let mut state = state.borrow_mut();
            state.native.initialize(init);
            state.ready = true;
            for operation in std::mem::take(&mut state.pending) {
                operation(&mut state.native);
            }
        };
        let ready = self.0.borrow().native.is_ready();
        if ready {
            initialize();
        } else {
            self.0.borrow().native.on_ready(initialize);
        }
    }

    fn call(&mut self, operation: impl FnOnce(&mut NativeWebview) + 'static) {
        let mut state = self.0.borrow_mut();
        if state.ready {
            operation(&mut state.native);
        } else {
            state.pending.push(Box::new(operation));
        }
    }
}

impl WebviewInterface for PlatformWebview {
    fn url(&self) -> Option<String> {
        let state = self.0.borrow();
        state.ready.then(|| state.native.url()).flatten()
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        let url = url.as_ref().to_owned();
        self.call(move |native| native.load_url(url));
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        let html = html.as_ref().to_owned();
        self.call(move |native| native.load_html(html));
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        let script = script.as_ref().to_owned();
        self.call(move |native| native.evaluate_script(script));
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: InjectionTime) {
        let script = script.as_ref().to_owned();
        self.call(move |native| native.add_user_script(script, injection_time));
    }

    fn set_background_color(&mut self, color: u32) {
        self.call(move |native| native.set_background_color(color));
    }
}
