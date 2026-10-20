/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::{env, mem};

use super::callback::{self, CallbackHandle, release_interface};
#[cfg(feature = "file_drop")]
use super::file_drop::{FileDropTarget, install_file_drop_targets};
use super::headers::*;
use super::loader::*;
#[cfg(feature = "custom_protocol")]
use crate::CustomProtocol;
use crate::{InjectionTime, WebviewBuilder, WebviewEvent};

pub(super) struct WebviewData {
    pub(super) attachment: crate::WindowAttachment,
    handler: Option<crate::EventHandler>,
    closed: Cell<bool>,
    pub(super) hwnd: HWND,
    pub(super) background_color: Cell<Option<u32>>,
    pub(super) should_load_url: Option<String>,
    pub(super) should_load_html: Option<String>,
    #[cfg(feature = "custom_protocol")]
    pub(super) custom_protocols: Vec<CustomProtocol>,
    pub(super) environment: Cell<Option<*mut ICoreWebView2Environment>>,
    pub(super) webview: Cell<Option<*mut ICoreWebView2>>,
    pub(super) controller: Cell<Option<*mut ICoreWebView2Controller>>,
    #[cfg(feature = "file_drop")]
    #[allow(clippy::vec_box)]
    // Registered COM pointers must remain stable when the vector grows.
    pub(super) drop_targets: RefCell<Vec<Box<FileDropTarget>>>,
}

pub(crate) struct PlatformWebview {
    pub(super) webview_data: Rc<WebviewData>,
}

impl PlatformWebview {
    pub(crate) fn new(attachment: crate::WindowAttachment) -> Self {
        let Some(crate::NativeWindowHandle::Win32(window)) =
            (unsafe { attachment.native_handle() })
        else {
            panic!("invalid native window handle");
        };
        let webview_data = Rc::new(WebviewData {
            closed: Cell::new(false),
            handler: None,
            hwnd: window,
            background_color: Cell::new(attachment.background_color()),
            should_load_url: None,
            should_load_html: None,
            #[cfg(feature = "custom_protocol")]
            custom_protocols: Vec::new(),
            environment: Cell::new(None),
            webview: Cell::new(None),
            controller: Cell::new(None),
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
                    QueryInterface: callback::query_interface,
                    AddRef: callback::add_ref,
                    Release: callback::release,
                    Invoke: environment_created,
                };
            let completed_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
                self.webview_data.clone(),
            );
            let result = create_core_webview2_environment(
                self.webview_data
                    .attachment
                    .windows_data_directory()
                    .display()
                    .to_string()
                    .to_wide_string()
                    .as_ptr() as *mut _,
                completed_handler.as_ptr(),
            );
            if result != S_OK {
                if result == WEBVIEW2_RUNTIME_NOT_FOUND {
                    if MessageBoxW(
                        self.webview_data.hwnd,
                        wide!("Microsoft Edge WebView2 Runtime is required to run this application.\n\nDownload it now?").as_ptr(),
                        wide!("WebView2 Runtime Required").as_ptr(),
                        MB_YESNO | MB_ICONWARNING,
                    ) == IDYES
                    {
                        ShellExecuteW(
                            self.webview_data.hwnd,
                            wide!("open").as_ptr(),
                            wide!("https://developer.microsoft.com/microsoft-edge/webview2/#download-section")
                                .as_ptr(),
                            null(),
                            null(),
                            SW_SHOWNORMAL,
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
        super::file_drop::revoke_file_drop_targets(self);
        if let Some(controller) = self.controller.take() {
            unsafe {
                (*controller).Close();
                release_interface(controller);
            }
        }
        if let Some(webview) = self.webview.take() {
            unsafe { release_interface(webview) };
        }
        if let Some(environment) = self.environment.take() {
            unsafe { release_interface(environment) };
        }
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
            if let Some(webview) = self.webview_data.webview.get() {
                let mut uri = LPWSTR::default();
                (*webview).get_Source(uri.as_mut_ptr());
                Some(uri.to_string())
            } else {
                None
            }
        }
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        unsafe {
            if let Some(webview) = self.webview_data.webview.get() {
                let url = cfg_select! {
                    feature = "custom_protocol" => replace_custom_protocol_in_url(
                        url.as_ref(),
                        &self.webview_data.custom_protocols,
                    ),
                    _ => url.as_ref(),
                };
                (*webview).Navigate(url.to_wide_string().as_ptr() as *mut _);
            }
        }
    }

    fn set_background_color(&mut self, color: u32) {
        self.webview_data.background_color.set(Some(color));
        unsafe {
            if let Some(controller) = self.webview_data.controller.get() {
                let mut controller2: *mut ICoreWebView2Controller2 = null_mut();
                (*controller).QueryInterface(
                    &IID_ICoreWebView2Controller2,
                    &mut controller2 as *mut _ as *mut *mut c_void,
                );
                if !controller2.is_null() {
                    (*controller2).put_DefaultBackgroundColor(COREWEBVIEW2_COLOR {
                        A: 0,
                        R: ((color >> 16) & 0xFF) as u8,
                        G: ((color >> 8) & 0xFF) as u8,
                        B: (color & 0xFF) as u8,
                    });
                    release_interface(controller2);
                }
            }
        }
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        unsafe {
            if let Some(webview) = self.webview_data.webview.get() {
                (*webview).NavigateToString(html.as_ref().to_wide_string().as_ptr() as *mut _);
            }
        }
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        unsafe {
            if let Some(webview) = self.webview_data.webview.get() {
                (*webview).ExecuteScript(
                    script.as_ref().to_wide_string().as_ptr() as *mut _,
                    null_mut(),
                );
            }
        }
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: InjectionTime) {
        let mut script = script.as_ref().to_string();
        unsafe {
            if let Some(webview) = self.webview_data.webview.get() {
                if let InjectionTime::DocumentLoaded = injection_time {
                    script = format!(
                        "window.addEventListener('DOMContentLoaded', function () {{ {script} }});"
                    );
                }
                (*webview).AddScriptToExecuteOnDocumentCreated(
                    script.to_wide_string().as_ptr() as *mut _,
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

        (*environment).AddRef();
        _self.environment.set(Some(environment));

        static VTBL: ICoreWebView2CreateCoreWebView2ControllerCompletedHandlerVtbl =
            ICoreWebView2CreateCoreWebView2ControllerCompletedHandlerVtbl {
                QueryInterface: callback::query_interface,
                AddRef: callback::add_ref,
                Release: callback::release,
                Invoke: controller_created,
            };
        let creation_completed_handler = CallbackHandle::new(
            &VTBL,
            IID_ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
            owner.clone(),
        );
        let mut controller_creation_started = false;
        if _self.background_color.get().is_some() {
            let mut environment10: *mut ICoreWebView2Environment10 = null_mut();
            if (*environment).QueryInterface(
                &IID_ICoreWebView2Environment10,
                &mut environment10 as *mut _ as *mut *mut c_void,
            ) == S_OK
            {
                let mut options: *mut ICoreWebView2ControllerOptions = null_mut();
                if (*environment10).CreateCoreWebView2ControllerOptions(&mut options) == S_OK {
                    let mut options3: *mut ICoreWebView2ControllerOptions3 = null_mut();
                    if (*options).QueryInterface(
                        &IID_ICoreWebView2ControllerOptions3,
                        &mut options3 as *mut _ as *mut *mut c_void,
                    ) == S_OK
                    {
                        // Start transparent so the native window background is visible from the
                        // first WebView2 frame, without a process-global environment variable.
                        if (*options3).put_DefaultBackgroundColor(COREWEBVIEW2_COLOR {
                            A: 0,
                            R: 0,
                            G: 0,
                            B: 0,
                        }) == S_OK
                        {
                            controller_creation_started =
                                (*environment10).CreateCoreWebView2ControllerWithOptions(
                                    _self.hwnd,
                                    options,
                                    creation_completed_handler.as_ptr(),
                                ) == S_OK;
                        }
                        (*options3).Release();
                    }
                    (*options).Release();
                }
                (*environment10).Release();
            }
        }

        if !controller_creation_started {
            (*environment)
                .CreateCoreWebView2Controller(_self.hwnd, creation_completed_handler.as_ptr());
        }

        S_OK
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
        (*controller).AddRef();
        _self.controller.set(Some(controller));
        super::accelerators::register(controller, owner.clone());

        // Keep partial WebView2 paints hidden behind the native backing surface.
        (*controller).put_IsVisible(FALSE);

        // Register resize callback on the window
        _self.attachment.on_resize(move |w, h| {
            (*controller).put_Bounds(RECT {
                left: 0,
                top: 0,
                right: w as i32,
                bottom: h as i32,
            });
        });

        // Set initial bounds
        let mut rect: RECT = mem::zeroed();
        GetClientRect(_self.hwnd, &mut rect);
        (*controller).put_Bounds(rect);

        // Let our drop target handle files instead of allowing WebView2 to
        // navigate to them.
        #[cfg(feature = "file_drop")]
        if _self.attachment.allow_file_drop() {
            let mut controller4: *mut ICoreWebView2Controller4 = null_mut();
            (*controller).QueryInterface(
                &IID_ICoreWebView2Controller4,
                &mut controller4 as *mut _ as *mut *mut c_void,
            );
            if !controller4.is_null() {
                (*controller4).put_AllowExternalDrop(FALSE);
                (*controller4).Release();
            }
            install_file_drop_targets(_self);
        }

        // Get webview
        let mut webview: *mut ICoreWebView2 = null_mut();
        (*controller).get_CoreWebView2(&mut webview);
        _self.webview.set(Some(webview));

        // Set transparent background if needed
        if _self.background_color.get().is_some() {
            let mut controller2: *mut ICoreWebView2Controller2 = null_mut();
            (*controller).QueryInterface(
                &IID_ICoreWebView2Controller2,
                &mut controller2 as *mut _ as *mut *mut c_void,
            );
            (*controller2).put_DefaultBackgroundColor(COREWEBVIEW2_COLOR {
                A: 0,
                R: 0,
                G: 0,
                B: 0,
            });
            release_interface(controller2);
        }

        // Set user agent
        let useragent = format!(
            "Mozilla/5.0 (Windows NT; {}) bwebview/{}",
            env::consts::ARCH,
            env!("CARGO_PKG_VERSION"),
        );
        let mut settings = null_mut();
        (*webview).get_Settings(&mut settings);

        let mut settings2: *mut ICoreWebView2Settings2 = null_mut();
        (*settings).QueryInterface(
            &IID_ICoreWebView2Settings2,
            &mut settings2 as *mut _ as *mut *mut c_void,
        );
        (*settings2).put_UserAgent(useragent.to_wide_string().as_ptr() as *mut _);
        release_interface(settings2);
        release_interface(settings);

        // Set custom protocols
        #[cfg(feature = "custom_protocol")]
        {
            for custom_protocol in &_self.custom_protocols {
                (*webview).AddWebResourceRequestedFilter(
                    format!("http://{}.localhost/*", custom_protocol.scheme)
                        .to_wide_string()
                        .as_ptr() as *mut _,
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                );
            }

            static VTBL: ICoreWebView2WebResourceRequestedEventHandlerVtbl =
                ICoreWebView2WebResourceRequestedEventHandlerVtbl {
                    QueryInterface: callback::query_interface,
                    AddRef: callback::add_ref,
                    Release: callback::release,
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
                    QueryInterface: callback::query_interface,
                    AddRef: callback::add_ref,
                    Release: callback::release,
                    Invoke: navigation_starting,
                };
            let navigation_starting_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2NavigationStartingEventHandler,
                owner.clone(),
            );
            (*webview).add_NavigationStarting(navigation_starting_handler.as_ptr(), null_mut());
        }
        {
            static VTBL: ICoreWebView2NavigationCompletedEventHandlerVtbl =
                ICoreWebView2NavigationCompletedEventHandlerVtbl {
                    QueryInterface: callback::query_interface,
                    AddRef: callback::add_ref,
                    Release: callback::release,
                    Invoke: navigation_completed,
                };
            let navigation_completed_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2NavigationCompletedEventHandler,
                owner.clone(),
            );
            (*webview).add_NavigationCompleted(navigation_completed_handler.as_ptr(), null_mut());
        }
        {
            static VTBL: ICoreWebView2DocumentTitleChangedEventHandlerVtbl =
                ICoreWebView2DocumentTitleChangedEventHandlerVtbl {
                    QueryInterface: callback::query_interface,
                    AddRef: callback::add_ref,
                    Release: callback::release,
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
                    QueryInterface: callback::query_interface,
                    AddRef: callback::add_ref,
                    Release: callback::release,
                    Invoke: new_window_requested,
                };
            let new_window_requested_handler = CallbackHandle::new(
                &VTBL,
                IID_ICoreWebView2NewWindowRequestedEventHandler,
                owner.clone(),
            );
            (*webview).add_NewWindowRequested(new_window_requested_handler.as_ptr(), null_mut());
        }

        // Setup ipc and console logging
        let script = super::super::IPC_SCRIPT;
        (*webview).AddScriptToExecuteOnDocumentCreated(
            script.to_wide_string().as_ptr() as *mut _,
            null_mut(),
        );

        static VTBL: ICoreWebView2WebMessageReceivedEventHandlerVtbl =
            ICoreWebView2WebMessageReceivedEventHandlerVtbl {
                QueryInterface: callback::query_interface,
                AddRef: callback::add_ref,
                Release: callback::release,
                Invoke: web_message_received,
            };
        let message_received_handler = CallbackHandle::new(
            &VTBL,
            IID_ICoreWebView2WebMessageReceivedEventHandler,
            owner.clone(),
        );
        (*webview).add_WebMessageReceived(message_received_handler.as_ptr(), null_mut());

        // Load initial contents
        if let Some(url) = &_self.should_load_url {
            let url = cfg_select! {
                feature = "custom_protocol" => {
                    replace_custom_protocol_in_url(url, &_self.custom_protocols)
                }
                _ => url.as_str(),
            };
            (*webview).Navigate(url.to_wide_string().as_ptr() as *mut _);
        }
        if let Some(html) = &_self.should_load_html {
            (*webview).NavigateToString(html.to_wide_string().as_ptr() as *mut _);
        }

        S_OK
    }
}

fn show_webview_error(hwnd: HWND, component: &str, result: HRESULT) {
    let message = format!(
        "Failed to create the WebView2 {component} (HRESULT 0x{:08X}).",
        result as u32
    )
    .to_wide_string();
    unsafe {
        MessageBoxW(
            hwnd,
            message.as_ptr(),
            wide!("WebView2 Error").as_ptr(),
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
    if let Some(controller) = _self.controller.get() {
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
        ShellExecuteW(
            null_mut(),
            wide!("open").as_ptr(),
            uri.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
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
        _self.emit(WebviewEvent::MessageReceive(message.to_string()));
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

    let mut webview2_request = null_mut();
    unsafe { (*args).get_Request(&mut webview2_request) };
    let http_request = webview2_request_to_http_request(webview2_request);
    unsafe { (*webview2_request).Release() };

    for custom_protocol in &_self.custom_protocols {
        if http_request.url.host() == Some(&format!("{}.localhost", custom_protocol.scheme)) {
            let response = (custom_protocol.handler)(&http_request);

            let webview2_response = http_response_to_webview2_response(
                response,
                _self.environment.get().expect("Should be some"),
            );
            unsafe { (*args).put_Response(webview2_response) };
            unsafe { (*webview2_response).Release() };

            return S_OK;
        }
    }
    panic!("No handler found for custom protocol");
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
    request: *mut ICoreWebView2WebResourceRequest,
) -> small_http::Request {
    unsafe {
        use std::str::FromStr;

        let mut method = LPWSTR::default();
        (*request).get_Method(method.as_mut_ptr());
        let method = method.to_string();

        let mut uri = LPWSTR::default();
        (*request).get_Uri(uri.as_mut_ptr());
        let uri = uri.to_string();

        let mut req = small_http::Request::with_method_and_url(
            small_http::Method::from_str(&method).unwrap_or(small_http::Method::Get),
            &uri,
        );
        {
            let mut headers = null_mut();
            (*request).get_Headers(&mut headers);
            let mut iterator = null_mut();
            (*headers).GetIterator(&mut iterator);
            let mut has_current: BOOL = FALSE;
            (*iterator).get_HasCurrentHeader(&mut has_current);
            while has_current == TRUE {
                let mut name = LPWSTR::default();
                let mut value = LPWSTR::default();
                (*iterator).GetCurrentHeader(name.as_mut_ptr(), value.as_mut_ptr());
                req = req.header(name.to_string(), value.to_string());
                (*iterator).MoveNext(&mut has_current);
            }
            (*iterator).Release();
            (*headers).Release();
        }
        {
            let mut body_stream = null_mut();
            (*request).get_Content(&mut body_stream);
            if !body_stream.is_null() {
                let mut stat: STATSTG = mem::zeroed();
                (*body_stream).Stat(&mut stat as *mut _, STATFLAG_NONAME);
                let size = stat.cbSize as usize;
                let mut buffer = vec![0u8; size];
                let mut read: u32 = 0;
                (*body_stream).Read(buffer.as_mut_ptr() as *mut c_void, size as u32, &mut read);
                req = req.body(buffer);
                (*body_stream).Release();
            }
        }
        req
    }
}

#[cfg(feature = "custom_protocol")]
fn http_response_to_webview2_response(
    response: small_http::Response,
    environment: *mut ICoreWebView2Environment,
) -> *mut ICoreWebView2WebResourceResponse {
    unsafe {
        let body_stream = SHCreateMemStream(response.body.as_ptr(), response.body.len() as u32);

        let mut webview2_response = null_mut();
        (*environment).CreateWebResourceResponse(
            body_stream,
            response.status as i32,
            response.status.to_string().to_wide_string().as_ptr() as *mut _,
            response
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}"))
                .collect::<Vec<_>>()
                .join("\n")
                .to_wide_string()
                .as_ptr() as *mut _,
            &mut webview2_response,
        );
        (*body_stream).Release();
        webview2_response
    }
}
