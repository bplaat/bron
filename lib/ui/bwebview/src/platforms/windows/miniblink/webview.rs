/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
#[cfg(feature = "custom_protocol")]
use std::ffi::c_char;
use std::ffi::{CString, c_void};
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::OnceLock;

#[cfg(feature = "file_drop")]
use super::super::com::ComPtr;
#[cfg(feature = "file_drop")]
use super::super::file_drop::{
    FileDropTarget, install_file_drop_targets, revoke_file_drop_targets,
};
#[cfg(feature = "custom_protocol")]
use super::super::headers::wide;
use super::super::headers::{GetClientRect, RECT};
use super::headers::{Api, Frame, JsState, JsValue, View, string};
use crate::{InjectionTime, TrustedUrl, WebviewBuilder, WebviewEvent, is_external_url};

const DATA_KEY: &std::ffi::CStr = c"bwebview";

struct WebviewData {
    api: &'static Api,
    attachment: bwindow::WindowAttachment,
    view: Cell<View>,
    trusted_url: TrustedUrl,
    handler: RefCell<Option<crate::EventHandler>>,
    scripts: RefCell<Vec<(String, InjectionTime)>>,
    #[cfg(feature = "custom_protocol")]
    protocols: RefCell<Vec<crate::CustomProtocol>>,
    #[cfg(feature = "file_drop")]
    // Registered COM interfaces must stay at stable addresses.
    drop_targets: RefCell<Vec<ComPtr<FileDropTarget>>>,
}

pub(crate) struct PlatformWebview(Rc<WebviewData>);

impl PlatformWebview {
    pub(crate) fn new(attachment: bwindow::WindowAttachment) -> Self {
        let api = Api::load();
        static ENGINE_THREAD: OnceLock<std::thread::ThreadId> = OnceLock::new();
        let thread = std::thread::current().id();
        let owner = ENGINE_THREAD.get_or_init(|| {
            unsafe {
                (api.wkeInitialize)();
                (api.wkeJsBindFunction)(
                    c"__bwebviewPostMessage".as_ptr(),
                    post_message,
                    null_mut(),
                    1,
                );
            }
            thread
        });
        assert_eq!(*owner, thread, "MiniBlink must stay on its creating thread");
        Self(Rc::new(WebviewData {
            api,
            attachment,
            view: Cell::new(null_mut()),
            trusted_url: TrustedUrl::default(),
            handler: RefCell::new(None),
            scripts: RefCell::new(Vec::new()),
            #[cfg(feature = "custom_protocol")]
            protocols: RefCell::new(Vec::new()),
            #[cfg(feature = "file_drop")]
            drop_targets: RefCell::new(Vec::new()),
        }))
    }

    pub(crate) fn init_webview(&mut self, builder: WebviewBuilder<'_>) {
        *self.0.handler.borrow_mut() = builder.event_handler;
        #[cfg(feature = "custom_protocol")]
        {
            *self.0.protocols.borrow_mut() = builder.custom_protocols;
        }
        let data = &self.0;
        let api = data.api;
        let Some(bwindow::NativeWindowHandle::Win32(hwnd)) =
            (unsafe { data.attachment.window_handle() })
        else {
            panic!("invalid native window handle");
        };
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) };
        let view = unsafe {
            // WKE_WINDOW_TYPE_CONTROL creates a child HWND with native input handling.
            (api.wkeCreateWebWindow)(2, hwnd, 0, 0, rect.right, rect.bottom)
        };
        assert!(!view.is_null(), "cannot create MiniBlink window");
        data.view.set(view);
        let param = Rc::as_ptr(data).cast_mut().cast();
        unsafe {
            (api.wkeSetUserKeyValue)(view, DATA_KEY.as_ptr(), param);
            (api.wkeOnURLChanged2)(view, Some(url_changed), param);
            (api.wkeOnTitleChanged)(view, Some(title_changed), param);
            (api.wkeOnDocumentReady2)(view, Some(document_ready), param);
            (api.wkeOnLoadingFinish)(view, Some(loading_finished), param);
            (api.wkeOnDidCreateScriptContext)(view, Some(script_context_created), param);
            (api.wkeOnCreateView)(view, Some(create_view), param);
            #[cfg(feature = "custom_protocol")]
            (api.wkeOnLoadUrlBegin)(view, Some(load_resource), param);
            let agent = format!("{} bwebview", string((api.wkeGetUserAgent)(view)));
            (api.wkeSetUserAgent)(view, cstring(&agent).as_ptr());
            (api.wkeSetDragEnable)(view, false);
            (api.wkeSetZoomFactor)(view, data.attachment.scale_factor());
            data.attachment
                .set_native_view(bwindow::NativeViewHandle::Win32((api.wkeGetWindowHandle)(
                    view,
                )));
            #[cfg(feature = "file_drop")]
            if data.attachment.allow_file_drop() {
                install_file_drop_targets(hwnd, data.attachment.event_sender(), &data.drop_targets);
            }
        }
        let weak = Rc::downgrade(data);
        data.attachment.on_scale_factor_changed(move |scale| {
            if let Some(data) = weak.upgrade()
                && !data.view.get().is_null()
            {
                unsafe { (data.api.wkeSetZoomFactor)(data.view.get(), scale) };
            }
        });
        let weak = Rc::downgrade(data);
        data.attachment.on_close(move || {
            if let Some(data) = weak.upgrade() {
                data.close();
            }
        });
        if let Some(url) = builder.should_load_url {
            crate::WebviewInterface::load_url(self, url);
        } else if let Some(html) = builder.should_load_html {
            crate::WebviewInterface::load_html(self, html);
        } else {
            crate::WebviewInterface::load_url(self, TrustedUrl::HTML_URL);
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

    fn run(&self, script: &str) {
        let view = self.view.get();
        if !view.is_null() {
            unsafe { (self.api.wkeRunJS)(view, cstring(script).as_ptr()) };
        }
    }

    fn close(&self) {
        let view = self.view.replace(null_mut());
        if view.is_null() {
            return;
        }
        self.attachment.disconnect();
        #[cfg(feature = "file_drop")]
        revoke_file_drop_targets(&self.drop_targets);
        unsafe {
            let api = self.api;
            // MiniBlink destroys views asynchronously. Remove every pointer into Rust state
            // before requesting destruction so queued callbacks cannot outlive this Rc.
            (api.wkeSetUserKeyValue)(view, DATA_KEY.as_ptr(), null_mut());
            (api.wkeOnURLChanged2)(view, None, null_mut());
            (api.wkeOnTitleChanged)(view, None, null_mut());
            (api.wkeOnDocumentReady2)(view, None, null_mut());
            (api.wkeOnLoadingFinish)(view, None, null_mut());
            (api.wkeOnDidCreateScriptContext)(view, None, null_mut());
            (api.wkeOnCreateView)(view, None, null_mut());
            #[cfg(feature = "custom_protocol")]
            (api.wkeOnLoadUrlBegin)(view, None, null_mut());
            (api.wkeDestroyWebWindow)(view);
        }
    }

    fn map_url(&self, url: &str) -> String {
        #[cfg(feature = "custom_protocol")]
        for protocol in self.protocols.borrow().iter() {
            if let Some(url) = map_protocol_url(url, &protocol.scheme) {
                return url;
            }
        }
        url.to_string()
    }
}

impl Drop for PlatformWebview {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl crate::WebviewInterface for PlatformWebview {
    fn url(&self) -> Option<String> {
        let view = self.0.view.get();
        (!view.is_null()).then(|| unsafe { string((self.0.api.wkeGetURL)(view)) })
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        let data = &self.0;
        if !data.view.get().is_null() {
            let url = data.map_url(url.as_ref());
            data.trusted_url.set(&url);
            unsafe { (data.api.wkeLoadURL)(data.view.get(), cstring(&url).as_ptr()) };
        }
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        let data = &self.0;
        if !data.view.get().is_null() {
            data.trusted_url.set(TrustedUrl::HTML_URL);
            // The default HTML loader generates a memory URL that would fail the IPC origin check.
            unsafe {
                (data.api.wkeLoadHtmlWithBaseUrl)(
                    data.view.get(),
                    cstring(html.as_ref()).as_ptr(),
                    c"about:blank".as_ptr(),
                )
            };
        }
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        self.0.run(script.as_ref());
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: InjectionTime) {
        self.0
            .scripts
            .borrow_mut()
            .push((script.as_ref().to_string(), injection_time));
    }

    fn set_background_color(&mut self, _color: u32) {
        // A transparent MiniBlink child window renders black, Windows only layers child
        // windows since Windows 8, so the page paints its own background.
    }
}

fn cstring(text: &str) -> CString {
    CString::new(text).expect("browser string contains a null character")
}

unsafe fn state<'a>(param: *mut c_void) -> Option<&'a WebviewData> {
    let data = unsafe { param.cast::<WebviewData>().as_ref() }?;
    (!data.view.get().is_null() && !data.attachment.is_closed()).then_some(data)
}

unsafe extern "C" fn script_context_created(
    view: View,
    param: *mut c_void,
    frame: Frame,
    _context: *mut c_void,
    _extension_group: i32,
    world: i32,
) {
    let Some(data) = (unsafe { state(param) }) else {
        return;
    };
    if world != 0 || unsafe { (data.api.wkeIsMainFrame)(view, frame) } == 0 {
        return;
    }
    let script = format!(
        "window.bwebview = {{ postMessage: __bwebviewPostMessage }};\n{}",
        crate::platforms::IPC_SCRIPT
    );
    unsafe { (data.api.wkeRunJsByFrame)(view, frame, cstring(&script).as_ptr(), false) };
    let scripts = data.scripts.borrow().clone();
    for (script, time) in scripts {
        if time == InjectionTime::DocumentStart {
            unsafe { (data.api.wkeRunJsByFrame)(view, frame, cstring(&script).as_ptr(), false) };
        }
    }
}

unsafe extern "C" fn document_ready(view: View, param: *mut c_void, frame: Frame) {
    let Some(data) = (unsafe { state(param) }) else {
        return;
    };
    if unsafe { (data.api.wkeIsMainFrame)(view, frame) } == 0 {
        return;
    }
    let scripts = data.scripts.borrow().clone();
    for (script, time) in scripts {
        if time == InjectionTime::DocumentLoaded {
            data.run(&script);
        }
    }
}

unsafe extern "C" fn url_changed(view: View, param: *mut c_void, frame: Frame, _url: *mut c_void) {
    if let Some(data) = unsafe { state(param) }
        && unsafe { (data.api.wkeIsMainFrame)(view, frame) } != 0
    {
        data.emit(WebviewEvent::PageLoadStart);
    }
}

unsafe extern "C" fn title_changed(_view: View, param: *mut c_void, title: *mut c_void) {
    if let Some(data) = unsafe { state(param) } {
        data.emit(WebviewEvent::PageTitleChange(unsafe {
            string((data.api.wkeGetString)(title))
        }));
    }
}

unsafe extern "C" fn loading_finished(
    view: View,
    param: *mut c_void,
    url: *mut c_void,
    _result: i32,
    _reason: *mut c_void,
) {
    if let Some(data) = unsafe { state(param) }
        && unsafe { string((data.api.wkeGetString)(url)) == string((data.api.wkeGetURL)(view)) }
    {
        data.emit(WebviewEvent::PageLoadFinish);
    }
}

unsafe extern "C" fn create_view(
    _view: View,
    _param: *mut c_void,
    _navigation: i32,
    url: *mut c_void,
    _features: *const c_void,
) -> View {
    let url = unsafe { string((Api::get().wkeGetString)(url)) };
    if is_external_url(&url) {
        bwindow::open_url(&url);
    }
    null_mut()
}

unsafe extern "C" fn post_message(es: JsState, _param: *mut c_void) -> JsValue {
    let api = Api::get();
    let undefined = unsafe { (api.jsUndefined)() };
    let view = unsafe { (api.jsGetWebView)(es) };
    if view.is_null() || unsafe { (api.jsArgCount)(es) } < 1 {
        return undefined;
    }
    let Some(data) = (unsafe { state((api.wkeGetUserKeyValue)(view, DATA_KEY.as_ptr())) }) else {
        return undefined;
    };
    // Check the calling frame's origin, rather than trusting the top-level URL when an
    // untrusted iframe calls the process-global native binding directly.
    // jsEval evaluates a function body, so the value must be returned explicitly.
    let source = unsafe {
        string((api.jsToTempString)(
            es,
            (api.jsEval)(es, c"return location.href;".as_ptr()),
        ))
    };
    if !data.trusted_url.allows(&source) {
        return undefined;
    }
    let message = unsafe { string((api.jsToTempString)(es, (api.jsArg)(es, 0))) };
    if let Some(message) = message.strip_prefix('i') {
        data.emit(WebviewEvent::MessageReceive(message.to_string()));
    }
    #[cfg(feature = "log")]
    if let Some(message) = message.strip_prefix('c') {
        let level = message.as_bytes().first().copied();
        let message = message.get(1..).unwrap_or_default();
        match level {
            Some(b'e') => log::error!("{message}"),
            Some(b'w') => log::warn!("{message}"),
            Some(b'i' | b'l') => log::info!("{message}"),
            Some(b'd') => log::debug!("{message}"),
            Some(b't') => log::trace!("{message}"),
            _ => {}
        }
    }
    undefined
}

#[cfg(feature = "custom_protocol")]
unsafe extern "C" fn load_resource(
    _view: View,
    param: *mut c_void,
    url: *const c_char,
    job: *mut c_void,
) -> bool {
    let Some(data) = (unsafe { state(param) }) else {
        return false;
    };
    let url = unsafe { string(url) };
    let Ok(url) = url.parse() else {
        return false;
    };
    let mut request = small_http::Request {
        url,
        ..Default::default()
    };
    let protocols = data.protocols.borrow();
    let Some(protocol) = protocols
        .iter()
        .find(|protocol| matches_protocol(&request, &protocol.scheme))
    else {
        return false;
    };
    let api = data.api;
    request.method = match unsafe { (api.wkeNetGetRequestMethod)(job) } {
        1 => small_http::Method::Get,
        2 => small_http::Method::Post,
        3 => small_http::Method::Put,
        _ => {
            unsafe { (api.wkeNetCancelRequest)(job) };
            return true;
        }
    };
    let mut header = unsafe { (api.wkeNetGetRawHttpHead)(job) };
    while let Some(value) = unsafe { header.as_ref() } {
        if let Some((name, value)) = unsafe { string(value.data) }.split_once(':') {
            request
                .headers
                .append(name.trim().to_string(), value.trim().to_string());
        }
        header = value.next;
    }
    let body = unsafe { (api.wkeNetGetPostBody)(job) };
    if let Some(elements) = unsafe { body.as_ref() } {
        let mut bytes = Vec::new();
        for index in 0..elements.length {
            let element = unsafe { &**elements.elements.add(index) };
            if element.kind != 0 {
                // Do not hand a partially decoded file upload to an application handler.
                unsafe {
                    (api.wkeNetFreePostBodyElements)(body);
                    (api.wkeNetCancelRequest)(job);
                }
                return true;
            }
            if let Some(buffer) = unsafe { element.data.as_ref() }
                && buffer.length != 0
            {
                bytes.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(buffer.data, buffer.length)
                });
            }
        }
        request.body = Some(bytes);
        unsafe { (api.wkeNetFreePostBodyElements)(body) };
    }
    let response = (protocol.handler)(&request);
    let mime = response
        .headers
        .get("Content-Type")
        .unwrap_or("application/octet-stream");
    unsafe {
        (api.wkeNetSetMIMEType)(job, cstring(mime).as_ptr());
        for (name, value) in response.headers.iter() {
            (api.wkeNetSetHTTPHeaderField)(job, wide(name).as_ptr(), wide(value).as_ptr(), true);
        }
        // This API fixes the status at 200 and substitutes a space for an empty body.
        (api.wkeNetSetData)(
            job,
            response.body.as_ptr().cast_mut().cast(),
            i32::try_from(response.body.len()).expect("MiniBlink response is too large"),
        );
    }
    true
}

#[cfg(feature = "custom_protocol")]
fn map_protocol_url(url: &str, scheme: &str) -> Option<String> {
    url.strip_prefix(&format!("{scheme}://"))
        .map(|path| format!("http://{scheme}.localhost/{path}"))
}

#[cfg(feature = "custom_protocol")]
fn matches_protocol(request: &small_http::Request, scheme: &str) -> bool {
    let url = &request.url;
    url.scheme() == "http"
        && url.userinfo().is_none()
        && matches!(url.port(), None | Some(80))
        && url
            .host()
            .is_some_and(|host| host.eq_ignore_ascii_case(&format!("{scheme}.localhost")))
}

#[cfg(all(test, feature = "custom_protocol"))]
mod tests {
    use super::*;

    #[test]
    fn maps_only_the_leading_custom_scheme() {
        assert_eq!(
            map_protocol_url("app://index.html?next=app://other", "app").as_deref(),
            Some("http://app.localhost/index.html?next=app://other")
        );
        assert_eq!(
            map_protocol_url("https://example.com/app://index.html", "app"),
            None
        );
    }

    #[test]
    fn intercepts_only_the_custom_protocol_origin() {
        for url in [
            "http://app.localhost/index.html",
            "http://APP.localhost:80/js/app.js",
        ] {
            assert!(matches_protocol(&small_http::Request::with_url(url), "app"));
        }
        for url in [
            "https://app.localhost/index.html",
            "http://app.localhost:8080/index.html",
            "http://app.localhost.example.com/index.html",
            "http://user@app.localhost/index.html",
        ] {
            assert!(!matches_protocol(
                &small_http::Request::with_url(url),
                "app"
            ));
        }
    }
}
