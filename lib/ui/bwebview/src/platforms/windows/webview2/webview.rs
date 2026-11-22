/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::rc::Rc;
use std::{env, mem};

use super::super::com::ComPtr;
#[cfg(feature = "file_drop")]
use super::super::file_drop::{FileDropTarget, install_file_drop_targets};
use super::callback::{self, CallbackHandle};
use super::headers::*;
use super::loader::*;
#[cfg(feature = "custom_protocol")]
use crate::CustomProtocol;
use crate::{InjectionTime, TrustedUrl, WebviewBuilder, WebviewEvent, is_external_url};

pub(super) struct WebviewData {
    pub(super) attachment: bwindow::WindowAttachment,
    handler: Option<crate::EventHandler>,
    closed: Cell<bool>,
    trusted_url: TrustedUrl,
    pub(super) hwnd: HWND,
    pub(super) background_color: Cell<Option<u32>>,
    pub(super) should_load_url: Option<String>,
    pub(super) should_load_html: Option<String>,
    #[cfg(feature = "custom_protocol")]
    pub(super) custom_protocols: Vec<CustomProtocol>,
    pub(super) environment: RefCell<Option<ComPtr<ICoreWebView2Environment>>>,
    pub(super) webview: RefCell<Option<ComPtr<ICoreWebView2>>>,
    pub(super) controller: RefCell<Option<ComPtr<ICoreWebView2Controller>>>,
    #[cfg(feature = "file_drop")]
    pub(super) drop_targets: RefCell<Vec<ComPtr<FileDropTarget>>>,
}

pub(crate) struct PlatformWebview {
    pub(super) webview_data: Rc<WebviewData>,
}

impl PlatformWebview {
    pub(crate) fn new(attachment: bwindow::WindowAttachment) -> Self {
        let Some(bwindow::NativeWindowHandle::Win32(window)) =
            (unsafe { attachment.window_handle() })
        else {
            panic!("invalid native window handle");
        };
        let webview_data = Rc::new(WebviewData {
            closed: Cell::new(false),
            trusted_url: TrustedUrl::default(),
            handler: None,
            hwnd: window,
            background_color: Cell::new(attachment.background_color()),
            should_load_url: None,
            should_load_html: None,
            #[cfg(feature = "custom_protocol")]
            custom_protocols: Vec::new(),
            environment: RefCell::new(None),
            webview: RefCell::new(None),
            controller: RefCell::new(None),
            #[cfg(feature = "file_drop")]
            drop_targets: RefCell::new(Vec::new()),
            attachment,
        });
        PlatformWebview { webview_data }
    }
}

impl PlatformWebview {
    pub(crate) fn init_webview(&mut self, builder: WebviewBuilder<'_>) {
        let data = Rc::get_mut(&mut self.webview_data).expect("webview already initialized");
        data.handler = builder.event_handler;
        data.should_load_url = builder.should_load_url;
        data.should_load_html = builder.should_load_html;
        #[cfg(feature = "custom_protocol")]
        {
            data.custom_protocols = builder.custom_protocols;
        }

        let weak = Rc::downgrade(&self.webview_data);
        self.webview_data.attachment.on_close(move || {
            if let Some(data) = weak.upgrade() {
                data.close();
            }
        });

        // Init Webview2 creation
        unsafe {
            static VTBL: ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandlerVtbl =
                ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandlerVtbl {
                    base: callback::IUNKNOWN,
                    Invoke: environment_created,
                };
            let completed_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
                self.webview_data.clone(),
            );
            let result = create_core_webview2_environment(
                wide(
                    &self
                        .webview_data
                        .attachment
                        .windows_data_directory()
                        .display()
                        .to_string(),
                )
                .as_ptr() as *mut _,
                completed_handler.as_ptr(),
            );
            if result != S_OK {
                if result == WEBVIEW2_RUNTIME_NOT_FOUND {
                    if MessageBoxW(
                        self.webview_data.hwnd,
                        w!("Microsoft Edge WebView2 Runtime is required to run this application.\n\nDownload it now?").as_ptr(),
                        w!("WebView2 Runtime Required").as_ptr(),
                        MB_YESNO | MB_ICONWARNING,
                    ) == IDYES
                    {
                        bwindow::open_url(
                            "https://developer.microsoft.com/microsoft-edge/webview2/#download-section",
                        );
                    }
                } else {
                    show_webview_error(self.webview_data.hwnd, "environment", result);
                }
                std::process::exit(1);
            }
        }
    }
}

impl WebviewData {
    fn emit(&self, event: WebviewEvent) {
        if let Some(handler) = &self.handler {
            handler(event);
        }
    }

    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.attachment.disconnect();
        #[cfg(feature = "file_drop")]
        super::super::file_drop::revoke_file_drop_targets(&self.drop_targets);
        if let Some(controller) = self.controller.take() {
            unsafe {
                controller.Close();
            }
        }
        drop(self.webview.take());
        drop(self.environment.take());
    }
}

impl Drop for PlatformWebview {
    fn drop(&mut self) {
        self.webview_data.close();
    }
}

impl crate::WebviewInterface for PlatformWebview {
    fn url(&self) -> Option<String> {
        unsafe {
            let webview = self.webview_data.webview.borrow().clone();
            if let Some(webview) = webview {
                let mut uri = LPWSTR::default();
                webview.get_Source(uri.as_mut_ptr());
                Some(uri.to_string())
            } else {
                None
            }
        }
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        unsafe {
            let webview = self.webview_data.webview.borrow().clone();
            if let Some(webview) = webview {
                let url = cfg_select! {
                    feature = "custom_protocol" => &replace_custom_protocol_in_url(
                        url.as_ref(),
                        &self.webview_data.custom_protocols,
                    ),
                    _ => url.as_ref(),
                };
                self.webview_data.trusted_url.set(url);
                webview.Navigate(wide(url).as_ptr() as *mut _);
            }
        }
    }

    fn set_background_color(&mut self, color: u32) {
        self.webview_data.background_color.set(Some(color));
        unsafe {
            let controller = self.webview_data.controller.borrow().clone();
            if let Some(controller) = controller
                && let Ok(controller2) = controller.query::<ICoreWebView2Controller2>()
            {
                controller2.put_DefaultBackgroundColor(COREWEBVIEW2_COLOR {
                    A: 0,
                    R: ((color >> 16) & 0xFF) as u8,
                    G: ((color >> 8) & 0xFF) as u8,
                    B: (color & 0xFF) as u8,
                });
            }
        }
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        unsafe {
            let webview = self.webview_data.webview.borrow().clone();
            if let Some(webview) = webview {
                self.webview_data.trusted_url.set(TrustedUrl::HTML_URL);
                webview.NavigateToString(wide(html.as_ref()).as_ptr() as *mut _);
            }
        }
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        unsafe {
            let webview = self.webview_data.webview.borrow().clone();
            if let Some(webview) = webview {
                webview.ExecuteScript(wide(script.as_ref()).as_ptr() as *mut _, null_mut());
            }
        }
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: InjectionTime) {
        let mut script = script.as_ref().to_string();
        unsafe {
            let webview = self.webview_data.webview.borrow().clone();
            if let Some(webview) = webview {
                if let InjectionTime::DocumentLoaded = injection_time {
                    script = format!(
                        "window.addEventListener('DOMContentLoaded', function () {{ {script} }});"
                    );
                }
                webview.AddScriptToExecuteOnDocumentCreated(
                    wide(&script).as_ptr() as *mut _,
                    null_mut(),
                );
            }
        }
    }
}

extern "system" fn environment_created(
    _this: *mut ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
    _result: HRESULT,
    environment: *mut ICoreWebView2Environment,
) -> HRESULT {
    unsafe {
        let owner = callback::state(_this.cast());
        let _self = &*owner;
        if _self.closed.get() || _self.attachment.is_closed() {
            return S_OK;
        }
        if _result != S_OK || environment.is_null() {
            show_webview_error(
                _self.hwnd,
                "environment",
                if _result == S_OK { E_POINTER } else { _result },
            );
            std::process::exit(1);
        }

        let environment = ComPtr::retain(environment).expect("validated environment");
        _self.environment.replace(Some(environment.clone()));

        static VTBL: ICoreWebView2CreateCoreWebView2ControllerCompletedHandlerVtbl =
            ICoreWebView2CreateCoreWebView2ControllerCompletedHandlerVtbl {
                base: callback::IUNKNOWN,
                Invoke: controller_created,
            };
        let creation_completed_handler = CallbackHandle::new(
            &VTBL,
            IID_ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
            owner.clone(),
        );
        let mut controller_creation_started = false;
        if _self.background_color.get().is_some()
            && let Ok(environment10) = environment.query::<ICoreWebView2Environment10>()
            && let Ok(options) =
                ComPtr::create(|out| environment10.CreateCoreWebView2ControllerOptions(out))
            && let Ok(options3) = options.query::<ICoreWebView2ControllerOptions3>()
            && options3.put_DefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 0,
                R: 0,
                G: 0,
                B: 0,
            }) == S_OK
        {
            // Start transparent without a process-global environment variable.
            controller_creation_started = environment10.CreateCoreWebView2ControllerWithOptions(
                _self.hwnd,
                options.as_ptr(),
                creation_completed_handler.as_ptr(),
            ) == S_OK;
        }

        if controller_creation_started || _self.closed.get() || _self.attachment.is_closed() {
            return S_OK;
        }
        let result = environment
            .CreateCoreWebView2Controller(_self.hwnd, creation_completed_handler.as_ptr());
        if result < 0 {
            _self.close();
            show_webview_error(_self.hwnd, "controller", result);
        }
        result
    }
}

extern "system" fn controller_created(
    _this: *mut ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
    _result: HRESULT,
    controller: *mut ICoreWebView2Controller,
) -> HRESULT {
    unsafe {
        let owner = callback::state(_this.cast());
        let _self = &*owner;
        if _self.closed.get() || _self.attachment.is_closed() {
            if !controller.is_null() {
                (*controller).Close();
            }
            return S_OK;
        }
        if _result != S_OK || controller.is_null() {
            show_webview_error(
                _self.hwnd,
                "controller",
                if _result == S_OK { E_POINTER } else { _result },
            );
            std::process::exit(1);
        }
        let controller = ComPtr::retain(controller).expect("validated controller");
        let webview = match ComPtr::create(|out| controller.get_CoreWebView2(out)) {
            Ok(webview) => webview,
            Err(error) => {
                controller.Close();
                _self.close();
                show_webview_error(_self.hwnd, "view", error);
                return error;
            }
        };
        let result = super::accelerators::register(controller.as_ptr(), owner.clone());
        if result < 0 {
            controller.Close();
            _self.close();
            show_webview_error(_self.hwnd, "accelerator handler", result);
            return result;
        }
        if _self.closed.get() || _self.attachment.is_closed() {
            controller.Close();
            return S_OK;
        }
        _self.webview.replace(Some(webview.clone()));
        _self.controller.replace(Some(controller.clone()));
        // Register resize callback on the window
        let weak = Rc::downgrade(&owner);
        _self.attachment.on_resize(move |w, h| {
            let Some(owner) = weak.upgrade() else { return };
            let controller = owner.controller.borrow().clone();
            let Some(controller) = controller.filter(|_| !owner.closed.get()) else {
                return;
            };
            controller.put_Bounds(RECT {
                left: 0,
                top: 0,
                right: w as i32,
                bottom: h as i32,
            });
        });

        // Keep partial WebView2 paints hidden behind the native backing surface.
        controller.put_IsVisible(FALSE);

        // Set initial bounds
        let mut rect: RECT = mem::zeroed();
        GetClientRect(_self.hwnd, &mut rect);
        controller.put_Bounds(rect);

        // Let our drop target handle files instead of allowing WebView2 to
        // navigate to them.
        #[cfg(feature = "file_drop")]
        if _self.attachment.allow_file_drop() {
            if let Ok(controller4) = controller.query::<ICoreWebView2Controller4>() {
                controller4.put_AllowExternalDrop(FALSE);
            }
            install_file_drop_targets(
                _self.hwnd,
                _self.attachment.event_sender(),
                &_self.drop_targets,
            );
        }

        // Set transparent background if needed
        if _self.background_color.get().is_some()
            && let Ok(controller2) = controller.query::<ICoreWebView2Controller2>()
        {
            controller2.put_DefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 0,
                R: 0,
                G: 0,
                B: 0,
            });
        }

        // Set user agent
        let useragent = format!(
            "Mozilla/5.0 (Windows NT; {}) bwebview/{}",
            env::consts::ARCH,
            env!("CARGO_PKG_VERSION"),
        );
        if let Ok(settings) = ComPtr::create(|out| webview.get_Settings(out))
            && let Ok(settings2) = settings.query::<ICoreWebView2Settings2>()
        {
            settings2.put_UserAgent(wide(&useragent).as_ptr() as *mut _);
        }

        // Set custom protocols
        #[cfg(feature = "custom_protocol")]
        {
            for custom_protocol in &_self.custom_protocols {
                webview.AddWebResourceRequestedFilter(
                    wide(&format!("http://{}.localhost/*", custom_protocol.scheme)).as_ptr()
                        as *mut _,
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                );
            }

            static VTBL: ICoreWebView2WebResourceRequestedEventHandlerVtbl =
                ICoreWebView2WebResourceRequestedEventHandlerVtbl {
                    base: callback::IUNKNOWN,
                    Invoke: web_resource_requested,
                };
            let web_resource_requested_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2WebResourceRequestedEventHandler,
                owner.clone(),
            );
            (*webview)
                .add_WebResourceRequested(web_resource_requested_handler.as_ptr(), null_mut());
        }

        // Setup event handlers
        {
            static VTBL: ICoreWebView2NavigationStartingEventHandlerVtbl =
                ICoreWebView2NavigationStartingEventHandlerVtbl {
                    base: callback::IUNKNOWN,
                    Invoke: navigation_starting,
                };
            let navigation_starting_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2NavigationStartingEventHandler,
                owner.clone(),
            );
            webview.add_NavigationStarting(navigation_starting_handler.as_ptr(), null_mut());
        }
        {
            static VTBL: ICoreWebView2NavigationCompletedEventHandlerVtbl =
                ICoreWebView2NavigationCompletedEventHandlerVtbl {
                    base: callback::IUNKNOWN,
                    Invoke: navigation_completed,
                };
            let navigation_completed_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2NavigationCompletedEventHandler,
                owner.clone(),
            );
            webview.add_NavigationCompleted(navigation_completed_handler.as_ptr(), null_mut());
        }
        {
            static VTBL: ICoreWebView2DocumentTitleChangedEventHandlerVtbl =
                ICoreWebView2DocumentTitleChangedEventHandlerVtbl {
                    base: callback::IUNKNOWN,
                    Invoke: document_title_changed,
                };
            let document_title_changed_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2DocumentTitleChangedEventHandler,
                owner.clone(),
            );
            (*webview)
                .add_DocumentTitleChanged(document_title_changed_handler.as_ptr(), null_mut());
        }
        {
            static VTBL: ICoreWebView2NewWindowRequestedEventHandlerVtbl =
                ICoreWebView2NewWindowRequestedEventHandlerVtbl {
                    base: callback::IUNKNOWN,
                    Invoke: new_window_requested,
                };
            let new_window_requested_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2NewWindowRequestedEventHandler,
                owner.clone(),
            );
            webview.add_NewWindowRequested(new_window_requested_handler.as_ptr(), null_mut());
        }

        // Setup ipc and console logging
        let script = crate::platforms::IPC_SCRIPT;
        webview.AddScriptToExecuteOnDocumentCreated(wide(script).as_ptr() as *mut _, null_mut());

        static VTBL: ICoreWebView2WebMessageReceivedEventHandlerVtbl =
            ICoreWebView2WebMessageReceivedEventHandlerVtbl {
                base: callback::IUNKNOWN,
                Invoke: web_message_received,
            };
        let message_received_handler = CallbackHandle::new(
            &VTBL,
            IID_ICoreWebView2WebMessageReceivedEventHandler,
            owner.clone(),
        );
        webview.add_WebMessageReceived(message_received_handler.as_ptr(), null_mut());

        // Load initial contents
        if let Some(url) = &_self.should_load_url {
            let url = cfg_select! {
                feature = "custom_protocol" => {
                    &replace_custom_protocol_in_url(url, &_self.custom_protocols)
                }
                _ => url.as_str(),
            };
            _self.trusted_url.set(url);
            webview.Navigate(wide(url).as_ptr() as *mut _);
        }
        if let Some(html) = &_self.should_load_html {
            _self.trusted_url.set(TrustedUrl::HTML_URL);
            webview.NavigateToString(wide(html).as_ptr() as *mut _);
        }

        S_OK
    }
}

fn show_webview_error(hwnd: HWND, component: &str, result: HRESULT) {
    let message = wide(&format!(
        "Failed to create the WebView2 {component} (HRESULT 0x{:08X}).",
        result as u32
    ));
    unsafe {
        MessageBoxW(
            hwnd,
            message.as_ptr(),
            w!("WebView2 Error").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

extern "system" fn navigation_starting(
    _this: *mut ICoreWebView2NavigationStartingEventHandler,
    _sender: *mut ICoreWebView2,
    _args: *mut ICoreWebView2NavigationStartingEventArgs,
) -> HRESULT {
    let owner = unsafe { callback::state(_this.cast()) };
    let _self = &*owner;
    if _self.closed.get() || _self.attachment.is_closed() {
        return S_OK;
    }
    _self.emit(WebviewEvent::PageLoadStart);
    S_OK
}

extern "system" fn navigation_completed(
    _this: *mut ICoreWebView2NavigationCompletedEventHandler,
    _sender: *mut ICoreWebView2,
    _args: *mut ICoreWebView2NavigationCompletedEventArgs,
) -> HRESULT {
    let owner = unsafe { callback::state(_this.cast()) };
    let _self = &*owner;
    if _self.closed.get() || _self.attachment.is_closed() {
        return S_OK;
    }
    let controller = _self.controller.borrow().clone();
    if let Some(controller) = controller {
        unsafe { (*controller).put_IsVisible(TRUE) };
    }
    _self.emit(WebviewEvent::PageLoadFinish);
    S_OK
}

extern "system" fn document_title_changed(
    _this: *mut ICoreWebView2DocumentTitleChangedEventHandler,
    _sender: *mut ICoreWebView2,
    _args: *mut c_void,
) -> HRESULT {
    let owner = unsafe { callback::state(_this.cast()) };
    let _self = &*owner;
    if _self.closed.get() || _self.attachment.is_closed() {
        return S_OK;
    }
    unsafe {
        let mut title = LPWSTR::default();
        (*_sender).get_DocumentTitle(title.as_mut_ptr());
        _self.emit(WebviewEvent::PageTitleChange(title.to_string()));
    }
    S_OK
}

extern "system" fn new_window_requested(
    _this: *mut ICoreWebView2NewWindowRequestedEventHandler,
    _sender: *mut ICoreWebView2,
    args: *mut ICoreWebView2NewWindowRequestedEventArgs,
) -> HRESULT {
    let owner = unsafe { callback::state(_this.cast()) };
    if owner.closed.get() || owner.attachment.is_closed() {
        return S_OK;
    }
    unsafe {
        (*args).put_Handled(TRUE);
        let mut uri = LPWSTR::default();
        (*args).get_Uri(uri.as_mut_ptr());
        let uri = uri.to_string();
        if is_external_url(&uri) {
            bwindow::open_url(&uri);
        }
    }
    S_OK
}

extern "system" fn web_message_received(
    _this: *mut ICoreWebView2WebMessageReceivedEventHandler,
    _sender: *mut ICoreWebView2,
    args: *mut ICoreWebView2WebMessageReceivedEventArgs,
) -> HRESULT {
    let owner = unsafe { callback::state(_this.cast()) };
    let _self = &*owner;
    if _self.closed.get() || _self.attachment.is_closed() {
        return S_OK;
    }
    let mut message = LPWSTR::default();
    unsafe { (*args).TryGetWebMessageAsString(message.as_mut_ptr()) };
    let message = message.to_string();
    let (r#type, message) = message.split_at(1);

    #[cfg(feature = "log")]
    if r#type == "c" {
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
    if r#type == "i" {
        let mut source = LPWSTR::default();
        unsafe { (*args).get_Source(source.as_mut_ptr()) };
        if _self.trusted_url.allows(&source.to_string()) {
            _self.emit(WebviewEvent::MessageReceive(message.to_string()));
        }
    }

    S_OK
}

#[cfg(feature = "custom_protocol")]
extern "system" fn web_resource_requested(
    _this: *mut ICoreWebView2WebResourceRequestedEventHandler,
    _sender: *mut ICoreWebView2,
    args: *mut ICoreWebView2WebResourceRequestedEventArgs,
) -> HRESULT {
    let owner = unsafe { callback::state(_this.cast()) };
    let _self = &*owner;
    if _self.closed.get() || _self.attachment.is_closed() {
        return S_OK;
    }

    let request = match unsafe { ComPtr::create(|out| (*args).get_Request(out)) } {
        Ok(request) => request,
        Err(error) => return error,
    };
    let http_request = match webview2_request_to_http_request(&request) {
        Ok(request) => request,
        Err(error) => return error,
    };

    for custom_protocol in &_self.custom_protocols {
        if http_request.url.host() == Some(&format!("{}.localhost", custom_protocol.scheme)) {
            let response = (custom_protocol.handler)(&http_request);

            // The handler can close the view synchronously.
            let environment = _self.environment.borrow().clone();
            let Some(environment) = environment.filter(|_| !_self.closed.get()) else {
                return S_OK;
            };
            return match http_response_to_webview2_response(response, &environment) {
                Ok(response) => unsafe { (*args).put_Response(response.as_ptr()) },
                Err(error) => error,
            };
        }
    }
    S_OK
}

#[cfg(feature = "custom_protocol")]
fn replace_custom_protocol_in_url(url: &str, custom_protocols: &[CustomProtocol]) -> String {
    for custom_protocol in custom_protocols {
        if url.starts_with(&format!("{}://", custom_protocol.scheme)) {
            return url.replace(
                &format!("{}://", custom_protocol.scheme),
                &format!("http://{}.localhost/", custom_protocol.scheme),
            );
        }
    }
    url.to_string()
}

#[cfg(feature = "custom_protocol")]
fn webview2_request_to_http_request(
    request: &ICoreWebView2WebResourceRequest,
) -> Result<small_http::Request, HRESULT> {
    use super::super::com::check;

    unsafe {
        use std::str::FromStr;

        let mut method = LPWSTR::default();
        check(request.get_Method(method.as_mut_ptr()))?;
        let mut uri = LPWSTR::default();
        check(request.get_Uri(uri.as_mut_ptr()))?;
        let mut req = small_http::Request {
            method: small_http::Method::from_str(&method.to_string())
                .unwrap_or(small_http::Method::Get),
            url: uri.to_string().parse().map_err(|_| E_INVALIDARG)?,
            ..small_http::Request::default()
        };
        if let Ok(headers) = ComPtr::create(|out| request.get_Headers(out))
            && let Ok(iterator) = ComPtr::create(|out| headers.GetIterator(out))
        {
            let mut has_current: BOOL = FALSE;
            check(iterator.get_HasCurrentHeader(&mut has_current))?;
            while has_current == TRUE {
                let mut name = LPWSTR::default();
                let mut value = LPWSTR::default();
                check(iterator.GetCurrentHeader(name.as_mut_ptr(), value.as_mut_ptr()))?;
                req = req.header(name.to_string(), value.to_string());
                check(iterator.MoveNext(&mut has_current))?;
            }
        }
        if let Ok(body_stream) = ComPtr::create(|out| request.get_Content(out)) {
            let mut stat: STATSTG = mem::zeroed();
            check(body_stream.Stat(&mut stat, STATFLAG_NONAME))?;
            let size = u32::try_from(stat.cbSize).map_err(|_| E_INVALIDARG)?;
            let mut buffer = Vec::new();
            buffer
                .try_reserve_exact(size as usize)
                .map_err(|_| E_OUTOFMEMORY)?;
            buffer.resize(size as usize, 0);
            let mut read = 0;
            if size != 0 {
                check(body_stream.Read(buffer.as_mut_ptr().cast(), size, &mut read))?;
                if read > size {
                    return Err(E_FAIL);
                }
                buffer.truncate(read as usize);
            }
            req = req.body(buffer);
        }
        Ok(req)
    }
}

#[cfg(feature = "custom_protocol")]
fn http_response_to_webview2_response(
    response: small_http::Response,
    environment: &ICoreWebView2Environment,
) -> Result<ComPtr<ICoreWebView2WebResourceResponse>, HRESULT> {
    let size = u32::try_from(response.body.len()).map_err(|_| E_INVALIDARG)?;
    unsafe {
        let body_stream = ComPtr::from_raw(SHCreateMemStream(response.body.as_ptr(), size))
            .ok_or(E_OUTOFMEMORY)?;
        ComPtr::create(|out| {
            environment.CreateWebResourceResponse(
                body_stream.as_ptr(),
                response.status as i32,
                wide(&response.status.to_string()).as_ptr() as *mut _,
                wide(
                    &response
                        .headers
                        .iter()
                        .map(|(name, value)| format!("{name}: {value}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .as_ptr() as *mut _,
                out,
            )
        })
    }
}
