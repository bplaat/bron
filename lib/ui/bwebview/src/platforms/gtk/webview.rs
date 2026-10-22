/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::env;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{null, null_mut};
use std::rc::Rc;

#[cfg(feature = "file_drop")]
use super::file_drop::{FileDropState, connect_signals};
use super::headers::*;
use crate::{InjectionTime, WebviewBuilder, WebviewEvent, WindowEvent};

pub(super) struct WebviewData {
    pub(super) attachment: crate::WindowAttachment,
    closed: Cell<bool>,
    handler: RefCell<Option<crate::EventHandler>>,
    manager: Cell<*mut WebKitUserContentManager>,
    pub(super) window: *mut GtkWindow,
    pub(super) background_color: Cell<Option<u32>>,
    #[cfg(feature = "file_drop")]
    pub(super) file_drop: RefCell<FileDropState>,
    pub(super) webview: Cell<*mut WebKitWebView>,
}

pub(crate) struct PlatformWebview(pub(super) Rc<WebviewData>);

const EDIT_COMMANDS: [&CStr; 6] = [c"Undo", c"Redo", c"Cut", c"Copy", c"Paste", c"SelectAll"];

unsafe fn install_edit_actions(view: *mut WebKitWebView) {
    extern "C" fn activate(action: *mut GSimpleAction, _: *mut GVariant, view: *mut WebKitWebView) {
        unsafe { webkit_web_view_execute_editing_command(view, g_action_get_name(action)) };
    }
    extern "C" fn changed(state: *mut WebKitEditorState, view: *mut WebKitWebView) {
        unsafe {
            let group = gtk_widget_get_action_group(view.cast(), c"bwindow-edit".as_ptr());
            for (name, enabled) in EDIT_COMMANDS.into_iter().zip([
                webkit_editor_state_is_undo_available(state),
                webkit_editor_state_is_redo_available(state),
                webkit_editor_state_is_cut_available(state),
                webkit_editor_state_is_copy_available(state),
                webkit_editor_state_is_paste_available(state),
                1,
            ]) {
                g_simple_action_set_enabled(
                    g_action_map_lookup_action(group, name.as_ptr()),
                    enabled,
                );
            }
        }
    }
    unsafe {
        let group = g_simple_action_group_new();
        for name in EDIT_COMMANDS {
            let action = g_simple_action_new(name.as_ptr(), null());
            g_signal_connect_object(
                action.cast(),
                c"activate".as_ptr(),
                activate as *const c_void,
                view.cast(),
                G_CONNECT_DEFAULT,
            );
            g_action_map_add_action(group, action);
            g_object_unref(action.cast());
        }
        gtk_widget_insert_action_group(view.cast(), c"bwindow-edit".as_ptr(), group);
        g_object_unref(group.cast());
        // Older WebKit versions have no editor-state change notification.
        let state = webkit_web_view_get_editor_state(view);
        if g_signal_lookup(
            c"changed".as_ptr(),
            g_type_from_name(c"WebKitEditorState".as_ptr()),
        ) != 0
        {
            changed(state, view);
            g_signal_connect_object(
                state.cast(),
                c"changed".as_ptr(),
                changed as *const c_void,
                view.cast(),
                G_CONNECT_DEFAULT,
            );
        }
    }
}

impl PlatformWebview {
    pub(crate) fn new(attachment: crate::WindowAttachment) -> Self {
        let Some(crate::NativeWindowHandle::Gtk(window)) = (unsafe { attachment.window_handle() })
        else {
            panic!("invalid native window handle");
        };
        PlatformWebview(Rc::new(WebviewData {
            closed: Cell::new(false),
            handler: RefCell::new(None),
            manager: Cell::new(null_mut()),
            window: window.cast(),
            background_color: Cell::new(attachment.background_color()),
            #[cfg(feature = "file_drop")]
            file_drop: RefCell::new(FileDropState::default()),
            webview: Cell::new(null_mut()),
            attachment,
        }))
    }
}

impl PlatformWebview {
    pub(crate) fn init_webview(&mut self, builder: WebviewBuilder<'_>) {
        let data = &*self.0;
        *data.handler.borrow_mut() = builder.event_handler;
        let weak = Rc::downgrade(&self.0);
        data.attachment.on_close(move || {
            if let Some(data) = weak.upgrade() {
                data.close();
            }
        });
        let is_wayland = unsafe { gdk_display_is_wayland(gdk_display_get_default()) };
        let window = data.window;

        // Create webview web context
        let web_context = unsafe {
            let web_context = webkit_web_context_get_default();

            #[cfg(feature = "custom_protocol")]
            for custom_protocol in builder.custom_protocols {
                extern "C" fn custom_protocol_destroy(data: *mut c_void) {
                    drop(unsafe {
                        use crate::CustomProtocol;
                        Box::from_raw(data as *mut CustomProtocol)
                    });
                }
                let scheme =
                    CString::new(custom_protocol.scheme.clone()).expect("Can't convert to CString");
                webkit_web_context_register_uri_scheme(
                    web_context,
                    scheme.as_ptr(),
                    webview_custom_uri_scheme as *const c_void,
                    Box::into_raw(Box::new(custom_protocol)) as *mut c_void,
                    custom_protocol_destroy as *const c_void,
                );
            }

            web_context
        };

        // Create webview user content manager
        let user_content_manager = unsafe {
            let script = super::super::IPC_SCRIPT;

            let user_content_manager = webkit_user_content_manager_new();
            data.manager.set(user_content_manager);
            let script = CString::new(script).expect("Can't convert to CString");
            let user_script = webkit_user_script_new(
                script.as_ptr(),
                WEBKIT_USER_CONTENT_INJECT_TOP_FRAME,
                WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START,
                null(),
                null(),
            );
            webkit_user_content_manager_add_script(user_content_manager, user_script);
            g_signal_connect_data(
                user_content_manager as *mut GObject,
                c"script-message-received::ipc".as_ptr(),
                webview_on_message_ipc as *const c_void,
                data as *const WebviewData as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            webkit_user_content_manager_register_script_message_handler(
                user_content_manager,
                c"ipc".as_ptr(),
            );
            #[cfg(feature = "log")]
            {
                g_signal_connect_data(
                    user_content_manager as *mut GObject,
                    c"script-message-received::console".as_ptr(),
                    webview_on_message_console as *const c_void,
                    data as *const WebviewData as *const c_void,
                    null(),
                    G_CONNECT_DEFAULT,
                );
                webkit_user_content_manager_register_script_message_handler(
                    user_content_manager,
                    c"console".as_ptr(),
                );
            }

            user_content_manager
        };

        // Create webview
        let webview = unsafe {
            let webview = g_object_new(
                webkit_web_view_get_type(),
                c"web-context".as_ptr(),
                web_context,
                c"user-content-manager".as_ptr(),
                user_content_manager,
                null::<c_void>(),
            ) as *mut WebKitWebView;
            gtk_container_add(window as *mut GtkWidget, webview as *mut GtkWidget);
            #[cfg(feature = "file_drop")]
            if data.attachment.allow_file_drop() {
                connect_signals(webview, data);
            }
            if data.background_color.get().is_some() {
                let rgba = GdkRGBA {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 0.0,
                };
                webkit_web_view_set_background_color(webview, &rgba);
            }

            let useragent = CString::new(format!(
                "Mozilla/5.0 ({}; {} {}) bwebview/{}",
                if is_wayland { "Wayland" } else { "X11" },
                if env::consts::OS == "linux" {
                    "Linux"
                } else {
                    env::consts::OS
                },
                env::consts::ARCH,
                env!("CARGO_PKG_VERSION"),
            ))
            .expect("Can't convert to CString");
            let webview_settings = webkit_web_view_get_settings(webview);
            webkit_settings_set_user_agent(webview_settings, useragent.as_ptr());
            if cfg!(debug_assertions) {
                webkit_settings_set_enable_developer_extras(webview_settings, true);
            }

            if let Some(should_load_url) = builder.should_load_url {
                let url = CString::new(should_load_url).expect("Can't convert to CString");
                webkit_web_view_load_uri(webview, url.as_ptr());
            }
            if let Some(should_load_html) = builder.should_load_html {
                let html = CString::new(should_load_html).expect("Can't convert to CString");
                webkit_web_view_load_html(webview, html.as_ptr(), null());
            }

            g_signal_connect_data(
                webview as *mut GObject,
                c"load-changed".as_ptr(),
                webview_on_load_changed as *const c_void,
                data as *const WebviewData as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            g_signal_connect_data(
                webview as *mut GObject,
                c"notify::title".as_ptr(),
                webview_on_title_changed as *const c_void,
                data as *const WebviewData as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            g_signal_connect_data(
                webview as *mut GObject,
                c"decide-policy".as_ptr(),
                webview_on_navigation_policy_decision as *const c_void,
                data as *const WebviewData as *const c_void,
                null(),
                G_CONNECT_DEFAULT,
            );
            webview
        };

        data.webview.set(webview);
        unsafe { install_edit_actions(webview) };
        unsafe { g_object_ref(webview.cast()) };

        // Show the native window but keep partial WebKit paints hidden
        unsafe {
            gtk_widget_show_all(data.window as *mut GtkWidget);
            gtk_widget_hide(data.webview.get() as *mut GtkWidget);
        }
    }
}

impl WebviewData {
    fn emit(&self, event: WebviewEvent) {
        let handler = self.handler.borrow().clone();
        if let Some(handler) = handler {
            handler(event);
        }
    }

    // Registered signal data originates from Rc<WebviewData>. Retain it across
    // callbacks because application code may drop the view while handling an event.
    pub(super) unsafe fn retain(data: &Self) -> Rc<Self> {
        let pointer = data as *const Self;
        unsafe {
            Rc::increment_strong_count(pointer);
            Rc::from_raw(pointer)
        }
    }

    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.attachment.disconnect();
        let view = self.webview.get();
        let manager = self.manager.get();
        let data = self as *const Self as *mut c_void;
        unsafe {
            if !manager.is_null() {
                g_signal_handlers_disconnect_matched(
                    manager.cast(),
                    16,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    data,
                );
            }
            if !view.is_null() {
                g_signal_handlers_disconnect_matched(
                    view.cast(),
                    16,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    data,
                );
                webkit_web_view_stop_loading(view);
                gtk_widget_destroy(view.cast());
            }
        }
    }
}

impl Drop for WebviewData {
    fn drop(&mut self) {
        self.close();
        if !self.webview.get().is_null() {
            unsafe { g_object_unref(self.webview.get().cast()) };
        }
        if !self.manager.get().is_null() {
            unsafe { g_object_unref(self.manager.get().cast()) };
        }
    }
}

impl Drop for PlatformWebview {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl crate::WebviewInterface for PlatformWebview {
    fn url(&self) -> Option<String> {
        if self.0.closed.get() {
            return None;
        }
        unsafe {
            let url = webkit_web_view_get_uri(self.0.webview.get());
            if !url.is_null() {
                Some(CStr::from_ptr(url).to_string_lossy().into_owned())
            } else {
                None
            }
        }
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        if self.0.closed.get() {
            return;
        }
        let url = CString::new(url.as_ref()).expect("Can't convert to CString");
        unsafe { webkit_web_view_load_uri(self.0.webview.get(), url.as_ptr()) }
    }

    fn set_background_color(&mut self, color: u32) {
        if self.0.closed.get() {
            return;
        }
        self.0.background_color.set(Some(color));
        unsafe {
            let rgba = GdkRGBA {
                red: ((color >> 16) & 0xFF) as f64 / 255.0,
                green: ((color >> 8) & 0xFF) as f64 / 255.0,
                blue: (color & 0xFF) as f64 / 255.0,
                alpha: 1.0,
            };
            webkit_web_view_set_background_color(self.0.webview.get(), &rgba);
        }
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        if self.0.closed.get() {
            return;
        }
        let html = CString::new(html.as_ref()).expect("Can't convert to CString");
        unsafe { webkit_web_view_load_html(self.0.webview.get(), html.as_ptr(), null()) }
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        if self.0.closed.get() {
            return;
        }
        let script = script.as_ref();
        unsafe {
            cfg_select! {
                webkit2gtk_4_1 => webkit_web_view_evaluate_javascript(
                    self.0.webview.get(),
                    script.as_ptr() as *const c_char,
                    script.len(),
                    null(),
                    null(),
                    null(),
                    null(),
                    null(),
                ),
                _ => {
                    let script = CString::new(script).expect("Can't convert to CString");
                    webkit_web_view_run_javascript(
                        self.0.webview.get(),
                        script.as_ptr(),
                        null(),
                        null(),
                        null(),
                    );
                }
            }
        }
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: InjectionTime) {
        if self.0.closed.get() {
            return;
        }
        let script = CString::new(script.as_ref()).expect("Can't convert to CString");
        unsafe {
            let user_content_manager =
                webkit_web_view_get_user_content_manager(self.0.webview.get());
            let user_script = webkit_user_script_new(
                script.as_ptr(),
                WEBKIT_USER_CONTENT_INJECT_TOP_FRAME,
                match injection_time {
                    InjectionTime::DocumentStart => WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START,
                    InjectionTime::DocumentLoaded => WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_END,
                },
                null(),
                null(),
            );
            webkit_user_content_manager_add_script(user_content_manager, user_script);
        }
    }
}

extern "C" fn webview_on_load_changed(
    webview: *mut WebKitWebView,
    event: i32,
    _self: &WebviewData,
) {
    let owner = unsafe { WebviewData::retain(_self) };
    let _self = &*owner;
    if _self.closed.get() {
        return;
    }
    if event == WEBKIT_LOAD_STARTED {
        _self.emit(WebviewEvent::PageLoadStart)
    }
    if event == WEBKIT_LOAD_FINISHED {
        unsafe { gtk_widget_show(webview as *mut GtkWidget) };
        _self.emit(WebviewEvent::PageLoadFinish)
    }
}

extern "C" fn webview_on_title_changed(
    webview: *mut WebKitWebView,
    _pspec: *const c_void,
    _self: &WebviewData,
) {
    let owner = unsafe { WebviewData::retain(_self) };
    let _self = &*owner;
    if _self.closed.get() {
        return;
    }
    let title = unsafe { webkit_web_view_get_title(webview) };
    let title = unsafe { CStr::from_ptr(title) }.to_string_lossy();
    _self.emit(WebviewEvent::PageTitleChange(title.to_string()));
}

extern "C" fn webview_on_navigation_policy_decision(
    _webview: *mut WebKitWebView,
    decision: *mut WebKitNavigationPolicyDecision,
    decision_type: i32,
    _self: &WebviewData,
) -> bool {
    let owner = unsafe { WebviewData::retain(_self) };
    let _self = &*owner;
    if _self.closed.get() {
        return false;
    }
    if decision_type == WEBKIT_POLICY_DECISION_TYPE_NEW_WINDOW_ACTION {
        let request = unsafe { webkit_navigation_policy_decision_get_request(decision) };
        let uri = unsafe { webkit_uri_request_get_uri(request) };
        cfg_select! {
            gtk3_22 => unsafe { gtk_show_uri_on_window(null_mut(), uri, 0, null_mut()) },
            _ => unsafe {
                _ = gtk_show_uri(gdk_screen_get_default(), uri, 0, null_mut());
            },
        }
        return true;
    }
    false
}

extern "C" fn webview_on_message_ipc(
    _manager: *mut WebKitUserContentManager,
    _message: *mut WebKitJavascriptResult,
    _self: &WebviewData,
) {
    let owner = unsafe { WebviewData::retain(_self) };
    let _self = &*owner;
    if _self.closed.get() {
        return;
    }
    let message = js_result_to_string(_message);
    _self.emit(WebviewEvent::MessageReceive(message));
}

#[cfg(feature = "log")]
extern "C" fn webview_on_message_console(
    _manager: *mut WebKitUserContentManager,
    _message: *mut WebKitJavascriptResult,
    _self: &WebviewData,
) {
    let owner = unsafe { WebviewData::retain(_self) };
    let _self = &*owner;
    if _self.closed.get() {
        return;
    }
    let message = js_result_to_string(_message);
    let (level, message) = message.split_at(1);
    match level {
        "e" => log::error!("{message}"),
        "w" => log::warn!("{message}"),
        "i" | "l" => log::info!("{message}"),
        "d" => log::debug!("{message}"),
        "t" => log::trace!("{message}"),
        _ => unimplemented!(),
    }
}

fn js_result_to_string(result: *mut WebKitJavascriptResult) -> String {
    cfg_select! {
        any(webkit2gtk_4_1, webkit2gtk_4_0_jsc_glib) => {
            let value = unsafe { webkit_javascript_result_get_js_value(result) };
            let s = unsafe { jsc_value_to_string(value) };
            unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
        }
        _ => {
            let ctx = unsafe { webkit_javascript_result_get_global_context(result) };
            let value = unsafe { webkit_javascript_result_get_value(result) };
            let js_str = unsafe { JSValueToStringCopy(ctx, value, null_mut()) };
            let max_size = unsafe { JSStringGetMaximumUTF8CStringSize(js_str) };
            let mut buf = vec![0u8; max_size];
            unsafe { JSStringGetUTF8CString(js_str, buf.as_mut_ptr() as *mut c_char, max_size) };
            unsafe { JSStringRelease(js_str) };
            unsafe { CStr::from_ptr(buf.as_ptr() as *const c_char) }
                .to_string_lossy()
                .into_owned()
        }
    }
}

#[cfg(feature = "custom_protocol")]
extern "C" fn webview_custom_uri_scheme(
    uri_scheme_request: *mut WebKitURISchemeRequest,
    custom_protocol: *mut crate::CustomProtocol,
) {
    let custom_protocol = unsafe { &mut *custom_protocol };
    let req = webkit_uri_scheme_request_to_http_request(uri_scheme_request);
    let res = (custom_protocol.handler)(&req);
    webkit_finish_uri_scheme_response(uri_scheme_request, res);
}

#[cfg(feature = "custom_protocol")]
fn webkit_uri_scheme_request_to_http_request(
    uri_scheme_request: *mut WebKitURISchemeRequest,
) -> small_http::Request {
    let uri = unsafe { webkit_uri_scheme_request_get_uri(uri_scheme_request) };
    let uri = unsafe { CStr::from_ptr(uri) }.to_string_lossy();

    cfg_select! {
        webkit2gtk_4_1 => {
            use std::str::FromStr;

            let method = unsafe { webkit_uri_scheme_request_get_http_method(uri_scheme_request) };
            let method = unsafe { CStr::from_ptr(method) }.to_string_lossy();
            let mut req = small_http::Request::with_method_and_url(
                small_http::Method::from_str(&method).unwrap_or(small_http::Method::Get),
                &uri,
            );

            let headers = unsafe { webkit_uri_scheme_request_get_http_headers(uri_scheme_request) };
            extern "C" fn headers_foreach(
                key: *const c_char,
                value: *const c_char,
                user_data: *mut c_void,
            ) {
                let req = unsafe { &mut *(user_data as *mut small_http::Request) };
                let key = unsafe { CStr::from_ptr(key) }.to_string_lossy();
                let value = unsafe { CStr::from_ptr(value) }.to_string_lossy();
                req.headers.insert(key.to_string(), value.to_string());
            }
            unsafe {
                soup_message_headers_foreach(
                    headers,
                    headers_foreach,
                    &mut req as *mut _ as *mut c_void,
                )
            };

            let body = unsafe { webkit_uri_scheme_request_get_http_body(uri_scheme_request) };
            if !body.is_null() {
                let mut body_data = Vec::new();
                let mut bytes_read = 0;
                let mut buffer = [0u8; 4096];
                loop {
                    let result = unsafe {
                        g_input_stream_read_all(
                            body,
                            buffer.as_mut_ptr() as *mut c_void,
                            buffer.len(),
                            &mut bytes_read,
                            null_mut(),
                            null_mut(),
                        )
                    };
                    if result || bytes_read == 0 {
                        break;
                    }
                    body_data.extend_from_slice(&buffer[..bytes_read]);
                    if bytes_read < buffer.len() {
                        break;
                    }
                }
                req = req.body(body_data);
            }

            req
        }
        // webkit2gtk-4.0 exposes only the URI, so every request is treated as GET.
        _ => small_http::Request::with_method_and_url(small_http::Method::Get, &uri),
    }
}

#[cfg(feature = "custom_protocol")]
fn webkit_finish_uri_scheme_response(
    uri_scheme_request: *mut WebKitURISchemeRequest,
    res: small_http::Response,
) {
    extern "C" fn body_data_destroy(data: *mut c_void) {
        drop(unsafe { Box::from_raw(data as *mut u8) });
    }
    let stream = unsafe {
        g_memory_input_stream_new_from_data(
            Box::into_raw(res.body.clone().into_boxed_slice()) as *const c_void,
            res.body.len(),
            body_data_destroy as *const c_void,
        )
    };

    cfg_select! {
        webkit2gtk_4_1 => {
            let uri_scheme_response =
                unsafe { webkit_uri_scheme_response_new(stream, res.body.len() as i64) };
            unsafe {
                webkit_uri_scheme_response_set_status(
                    uri_scheme_response,
                    res.status as u32,
                    null(),
                )
            };
            let headers = unsafe {
                let headers = soup_message_headers_new(SOUP_MESSAGE_HEADERS_RESPONSE);
                for (key, value) in &res.headers {
                    let key = CString::new(key.as_str()).expect("Can't convert to CString");
                    let value = CString::new(value.as_str()).expect("Can't convert to CString");
                    soup_message_headers_append(headers, key.as_ptr(), value.as_ptr());
                }
                headers
            };
            unsafe { webkit_uri_scheme_response_set_http_headers(uri_scheme_response, headers) };
            if let Some(content_type) = res.headers.get("Content-Type") {
                let content_type = CString::new(content_type).expect("Can't convert to CString");
                unsafe {
                    webkit_uri_scheme_response_set_content_type(
                        uri_scheme_response,
                        content_type.as_ptr(),
                    )
                };
            }
            unsafe {
                webkit_uri_scheme_request_finish_with_response(
                    uri_scheme_request,
                    uri_scheme_response,
                );
                g_object_unref(uri_scheme_response as *mut GObject);
            }
        }
        _ => {
            let content_type = res
                .headers
                .get("Content-Type")
                .map(|ct| CString::new(ct).expect("Can't convert to CString"));
            unsafe {
                webkit_uri_scheme_request_finish(
                    uri_scheme_request,
                    stream,
                    res.body.len() as i64,
                    content_type.as_ref().map_or(null(), |ct| ct.as_ptr()),
                );
                g_object_unref(stream as *mut GObject);
            }
        }
    }
}
