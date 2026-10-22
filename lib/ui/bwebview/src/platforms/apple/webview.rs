/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;

use block2::Block;
#[cfg(all(target_os = "macos", feature = "file_drop"))]
use bwindow::ffi::register_dragged_types;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, define_class, msg_send};

#[cfg(all(target_os = "macos", feature = "file_drop"))]
use super::file_drop::droppable_webview_class;
use super::headers::*;
use crate::{InjectionTime, WebviewBuilder, WebviewEvent};

fn title_key_path() -> *mut Object {
    ns_string!("title")
}

fn ipc_handler_name() -> *mut Object {
    ns_string!("ipc")
}

#[cfg(feature = "log")]
fn console_handler_name() -> *mut Object {
    ns_string!("console")
}

type WebviewDelegateIvars = Option<crate::EventHandler>;

// MARK: WebviewDelegate
define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = WebviewDelegateIvars]
    struct WebviewDelegate;

    impl WebviewDelegate {
        #[unsafe(method(webView:didStartProvisionalNavigation:))]
        fn _did_start_provisional_navigation(&self, _: *mut Object, _: *mut Object) { self.did_start_provisional_navigation(); }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn _did_finish_navigation(&self, webview: *mut Object, _: *mut Object) { self.finish_navigation(webview); }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn _did_fail_provisional_navigation(&self, webview: *mut Object, _: *mut Object, _: *mut Object) { self.finish_navigation(webview); }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn _did_fail_navigation(&self, webview: *mut Object, _: *mut Object, _: *mut Object) { self.finish_navigation(webview); }

        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn _observe_value(&self, key_path: &Object, _: *mut Object, change: *mut Object, _: *mut c_void) {
            self.observe_value(key_path, change);
        }

        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn _decide_policy(&self, _: *mut Object, navigation_action: *mut Object, decision_handler: &Block<dyn Fn(i64)>) {
            self.decide_policy(navigation_action, decision_handler);
        }

        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn _did_receive_script_message(&self, _: *mut Object, message: *mut Object) {
            self.did_receive_script_message(message);
        }
    }
);

impl WebviewDelegate {
    fn emit(&self, event: WebviewEvent) {
        if let Some(handler) = self.ivars() {
            handler(event);
        }
    }

    fn did_start_provisional_navigation(&self) {
        self.emit(WebviewEvent::PageLoadStart);
    }

    fn finish_navigation(&self, webview: *mut Object) {
        // Reveal successful loads and WebKit error pages alike. Otherwise a failed
        // initial navigation would leave the window on its backing color forever.
        let _: () = unsafe { msg_send![webview, setHidden:Bool::NO] };
        self.emit(WebviewEvent::PageLoadFinish);
    }

    fn observe_value(&self, key_path: &Object, change: *mut Object) {
        let key_path: NSString = unsafe { msg_send![key_path, copy] };
        if key_path.to_string() == "title" {
            let change: NSString =
                unsafe { msg_send![change, objectForKey:NSKeyValueChangeNewKey] };
            self.emit(WebviewEvent::PageTitleChange(change.to_string()));
        }
    }

    fn decide_policy(&self, navigation_action: *mut Object, decision_handler: &Block<dyn Fn(i64)>) {
        unsafe {
            let target_frame: *mut Object = msg_send![navigation_action, targetFrame];
            if target_frame.is_null() {
                let request: *mut Object = msg_send![navigation_action, request];
                let url: *mut Object = msg_send![request, URL];
                open_external_url(url);
                decision_handler.call((WK_NAVIGATION_ACTION_POLICY_CANCEL,));
            } else {
                decision_handler.call((WK_NAVIGATION_ACTION_POLICY_ALLOW,));
            }
        }
    }

    fn did_receive_script_message(&self, message: *mut Object) {
        let name: NSString = unsafe { msg_send![message, name] };
        let name = name.to_string();
        let body: NSString = unsafe { msg_send![message, body] };
        let body = body.to_string();

        #[cfg(feature = "log")]
        if name == "console" {
            let mut body = body.chars();
            let Some(level) = body.next() else {
                return;
            };
            let message = body.as_str();
            match level {
                'e' => log::error!("{message}"),
                'w' => log::warn!("{message}"),
                'i' | 'l' => log::info!("{message}"),
                'd' => log::debug!("{message}"),
                't' => log::trace!("{message}"),
                _ => return,
            }
        }
        if name == "ipc" {
            self.emit(WebviewEvent::MessageReceive(body));
        }
    }
}

pub(super) struct WebviewData {
    attachment: crate::WindowAttachment,
    closed: Rc<Cell<bool>>,
    pub(super) window: Retained<Object>,
    pub(super) background_color: Option<u32>,
    pub(super) webview: Option<Retained<Object>>,
    delegate: Option<Retained<Object>>,
    #[cfg(feature = "custom_protocol")]
    protocol_delegates: Vec<Retained<Object>>,
}

pub(crate) struct PlatformWebview(pub(super) Box<WebviewData>);

impl PlatformWebview {
    pub(crate) fn new(attachment: crate::WindowAttachment) -> Self {
        #[cfg(target_os = "macos")]
        let Some(crate::NativeWindowHandle::AppKit(window)) =
            (unsafe { attachment.native_handle() })
        else {
            panic!("invalid native window handle");
        };
        // On iOS the window handle is the root view that content is added to.
        #[cfg(target_os = "ios")]
        let Some(crate::NativeWindowHandle::UIKit(window)) =
            (unsafe { attachment.native_handle() })
        else {
            panic!("invalid native window handle");
        };
        PlatformWebview(Box::new(WebviewData {
            closed: Rc::new(Cell::new(false)),
            window: unsafe { Retained::retain(window.cast()) }.expect("window has been destroyed"),
            background_color: attachment.background_color(),
            webview: None,
            delegate: None,
            attachment,
            #[cfg(feature = "custom_protocol")]
            protocol_delegates: Vec::new(),
        }))
    }

    fn webview(&self) -> &Retained<Object> {
        self.0.webview.as_ref().expect("webview is not initialized")
    }
}

impl PlatformWebview {
    pub(crate) fn init_webview(&mut self, mut builder: WebviewBuilder<'_>) {
        // Create WebviewDelegate instance (registers class lazily on first call)
        let delegate: Allocated<WebviewDelegate> =
            unsafe { msg_send![WebviewDelegate::class(), alloc] };
        let webview_delegate: Retained<Object> = unsafe {
            msg_send![
                super(delegate.set_ivars(builder.event_handler.take())),
                init
            ]
        };

        // Create webview
        let webview = unsafe {
            // Create webview configuration
            let webview_config: Retained<Object> = msg_send![class!(WKWebViewConfiguration), new];
            let website_data_store: *mut Object =
                msg_send![class!(WKWebsiteDataStore), defaultDataStore];
            let _: () = msg_send![&webview_config, setWebsiteDataStore:website_data_store];

            #[cfg(feature = "custom_protocol")]
            for custom_protocol in builder.custom_protocols {
                let url_scheme = NSString::new(&custom_protocol.scheme);
                let delegate: Allocated<CustomProtocolDelegate> =
                    msg_send![CustomProtocolDelegate::class(), alloc];
                let delegate: Retained<CustomProtocolDelegate> = msg_send![
                    super(delegate.set_ivars(CustomProtocolDelegateIvars { custom_protocol })),
                    init
                ];
                let _: () = msg_send![&webview_config, setURLSchemeHandler:delegate.as_ptr(), forURLScheme:&*url_scheme];
                self.0.protocol_delegates.push(Retained::into_any(delegate));
            }

            // Get content view rect
            #[cfg(target_os = "macos")]
            let content_view: *mut Object = msg_send![&self.0.window, contentView];
            #[cfg(target_os = "ios")]
            let content_view: *mut Object = self.0.window.as_ptr();
            let webview_rect: CGRect = msg_send![content_view, bounds];

            // Create webview
            #[cfg(all(target_os = "macos", feature = "file_drop"))]
            let webview_class = if self.0.attachment.allow_file_drop() {
                droppable_webview_class()
            } else {
                class!(WKWebView)
            };
            #[cfg(not(all(target_os = "macos", feature = "file_drop")))]
            let webview_class = class!(WKWebView);
            let webview: Allocated<Object> = msg_send![webview_class, alloc];
            let webview: Retained<Object> = msg_send![webview, initWithFrame:webview_rect, configuration:webview_config.as_ptr()];
            #[cfg(all(target_os = "macos", feature = "file_drop"))]
            if self.0.attachment.allow_file_drop() {
                register_dragged_types(webview.as_ptr());
            }
            let _: () = msg_send![&webview, setHidden:Bool::YES];
            let _: () = msg_send![&webview, setNavigationDelegate:webview_delegate.as_ptr()];
            #[cfg(target_os = "macos")]
            let _: () = msg_send![content_view, addSubview:webview.as_ptr(), positioned:NS_WINDOW_BELOW, relativeTo:null_mut::<Object>()];
            #[cfg(target_os = "ios")]
            let _: () = msg_send![content_view, insertSubview:webview.as_ptr(), atIndex:0usize];
            #[cfg(target_os = "macos")]
            let _: () = msg_send![&webview, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
            #[cfg(target_os = "ios")]
            let _: () = msg_send![&webview, setAutoresizingMask: UI_VIEW_AUTORESIZING_FLEXIBLE_WIDTH | UI_VIEW_AUTORESIZING_FLEXIBLE_HEIGHT];
            if self.0.background_color.is_some() {
                set_transparent_background(&webview);
            }
            // The device model is iPhone or iPad on iOS
            #[cfg(target_os = "ios")]
            let device = {
                let device: *mut Object = msg_send![class!(UIDevice), currentDevice];
                let model: NSString = msg_send![device, model];
                model.to_string()
            };
            #[cfg(target_os = "macos")]
            let device = "Macintosh";
            let useragent = format!(
                "Mozilla/5.0 ({device}; {}) bwebview/{}",
                std::env::consts::ARCH,
                env!("CARGO_PKG_VERSION"),
            );
            let _: () = msg_send![&webview, setCustomUserAgent:&*NSString::new(&useragent)];
            let _: () = msg_send![
                &webview,
                addObserver:webview_delegate.as_ptr(),
                forKeyPath:ns_string!("title"),
                options:NS_KEY_VALUE_OBSERVING_OPTION_NEW,
                context:null::<c_void>()
            ];
            if let Some(url) = builder.should_load_url {
                let url: *mut Object = msg_send![class!(NSURL), URLWithString:&*NSString::new(url)];
                let request: *mut Object = msg_send![class!(NSURLRequest), requestWithURL:url];
                let _: *mut Object = msg_send![&webview, loadRequest:request];
            }
            if let Some(html) = builder.should_load_html {
                let _: *mut Object = msg_send![&webview, loadHTMLString:&*NSString::new(html), baseURL:null::<Object>()];
            }
            // isInspectable is available since iOS 16.4
            #[cfg(target_os = "ios")]
            if cfg!(debug_assertions) {
                let inspectable: Bool =
                    msg_send![&webview, respondsToSelector:objc2::sel!(setInspectable:)];
                if inspectable.as_bool() {
                    let _: () = msg_send![&webview, setInspectable:Bool::YES];
                }
            }
            #[cfg(target_os = "macos")]
            if cfg!(debug_assertions) {
                let webview_configuration: *mut Object = msg_send![&webview, configuration];
                let webview_preferences: *mut Object =
                    msg_send![webview_configuration, preferences];
                let ns_bool_yes: *mut Object =
                    msg_send![class!(NSNumber), numberWithBool:Bool::YES];
                let _: () = msg_send![webview_preferences, setValue:ns_bool_yes, forKey:ns_string!("developerExtrasEnabled")];
            }
            webview
        };

        // Create ipc handler
        unsafe {
            let script = super::super::IPC_SCRIPT;

            let webview_configuration: *mut Object = msg_send![&webview, configuration];
            let user_content_controller: *mut Object =
                msg_send![webview_configuration, userContentController];
            let user_script: Allocated<Object> = msg_send![class!(WKUserScript), alloc];
            let user_script: Retained<Object> = msg_send![user_script,
                    initWithSource:&*NSString::new(script),
                    injectionTime:WK_USER_SCRIPT_INJECTION_TIME_AT_DOCUMENT_START,
                    forMainFrameOnly:Bool::YES];
            let _: () = msg_send![user_content_controller, addUserScript:user_script.as_ptr()];
            let _: () = msg_send![user_content_controller, addScriptMessageHandler:webview_delegate.as_ptr(), name:ns_string!("ipc")];
            #[cfg(feature = "log")]
            let _: () = msg_send![user_content_controller, addScriptMessageHandler:webview_delegate.as_ptr(), name:ns_string!("console")];
        }

        let content = webview.clone();
        let delegate = webview_delegate.clone();
        let closed = self.0.closed.clone();
        self.0.attachment.on_close(move || {
            if !closed.replace(true) {
                close_webview(&content, &delegate);
            }
        });
        self.0.webview = Some(webview);
        self.0.delegate = Some(webview_delegate);
    }
}

impl crate::WebviewInterface for PlatformWebview {
    fn url(&self) -> Option<String> {
        if self.0.closed.get() {
            return None;
        }
        unsafe {
            let url: *mut Object = msg_send![self.webview(), URL];
            if !url.is_null() {
                let url: NSString = msg_send![url, absoluteString];
                Some(url.to_string())
            } else {
                None
            }
        }
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        if self.0.closed.get() {
            return;
        }
        unsafe {
            let url: *mut Object = msg_send![class!(NSURL), URLWithString:&*NSString::new(url)];
            let request: *mut Object = msg_send![class!(NSURLRequest), requestWithURL:url];
            msg_send![self.webview(), loadRequest:request]
        }
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        if self.0.closed.get() {
            return;
        }
        unsafe {
            msg_send![self.webview(), loadHTMLString:&*NSString::new(html), baseURL:null::<c_void>()]
        }
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        if self.0.closed.get() {
            return;
        }
        let script = script.as_ref();
        let _: () = unsafe {
            msg_send![self.webview(), evaluateJavaScript:&*NSString::new(script), completionHandler:null::<Object>()]
        };
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: InjectionTime) {
        if self.0.closed.get() {
            return;
        }
        let script = script.as_ref();
        unsafe {
            let webview_configuration: *mut Object = msg_send![self.webview(), configuration];
            let user_content_controller: *mut Object =
                msg_send![webview_configuration, userContentController];
            let user_script: Allocated<Object> = msg_send![class!(WKUserScript), alloc];
            let user_script: Retained<Object> = msg_send![user_script,
                    initWithSource:&*NSString::new(script),
                    injectionTime: match injection_time {
                        InjectionTime::DocumentStart => WK_USER_SCRIPT_INJECTION_TIME_AT_DOCUMENT_START,
                        InjectionTime::DocumentLoaded => WK_USER_SCRIPT_INJECTION_TIME_AT_DOCUMENT_END,
                    },
                    forMainFrameOnly:Bool::YES];
            let _: () = msg_send![user_content_controller, addUserScript:user_script.as_ptr()];
        }
    }

    fn set_background_color(&mut self, color: u32) {
        if self.0.closed.get() {
            return;
        }
        self.0.background_color = Some(color);
        if let Some(webview) = &self.0.webview {
            set_transparent_background(webview);
        }
    }
}

/// Let the window background color show through the webview
fn set_transparent_background(webview: &Object) {
    unsafe {
        #[cfg(target_os = "macos")]
        {
            let value: *mut Object = msg_send![class!(NSNumber), numberWithBool:Bool::NO];
            let _: () = msg_send![webview, setValue:value, forKey:ns_string!("drawsBackground")];
        }
        #[cfg(target_os = "ios")]
        {
            let clear: *mut Object = msg_send![class!(UIColor), clearColor];
            let _: () = msg_send![webview, setOpaque:Bool::NO];
            let _: () = msg_send![webview, setBackgroundColor:clear];
            let scroll_view: *mut Object = msg_send![webview, scrollView];
            let _: () = msg_send![scroll_view, setBackgroundColor:clear];
        }
    }
}

fn open_external_url(url: *mut Object) {
    unsafe {
        #[cfg(target_os = "macos")]
        {
            let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            let _: Bool = msg_send![workspace, openURL:url];
        }
        #[cfg(target_os = "ios")]
        {
            let application: *mut Object = msg_send![class!(UIApplication), sharedApplication];
            let options: *mut Object = msg_send![class!(NSDictionary), dictionary];
            let _: () = msg_send![application, openURL:url, options:options, completionHandler:null_mut::<c_void>()];
        }
    }
}

fn close_webview(webview: &Object, delegate: &Object) {
    let title = title_key_path();
    let ipc = ipc_handler_name();
    #[cfg(feature = "log")]
    let console = console_handler_name();
    unsafe {
        let _: () = msg_send![webview, removeObserver:delegate, forKeyPath:title];
        let configuration: *mut Object = msg_send![webview, configuration];
        let controller: *mut Object = msg_send![configuration, userContentController];
        let _: () = msg_send![controller, removeScriptMessageHandlerForName:ipc];
        #[cfg(feature = "log")]
        let _: () = msg_send![controller, removeScriptMessageHandlerForName:console];
        let _: () = msg_send![webview, setNavigationDelegate:null_mut::<Object>()];
        let _: () = msg_send![webview, stopLoading];
        let _: () = msg_send![webview, removeFromSuperview];
    }
}

impl Drop for PlatformWebview {
    fn drop(&mut self) {
        self.0.attachment.disconnect();
        if self.0.closed.replace(true) {
            return;
        }
        if let (Some(webview), Some(delegate)) = (&self.0.webview, &self.0.delegate) {
            close_webview(webview, delegate);
        }
    }
}

#[cfg(feature = "custom_protocol")]
struct CustomProtocolDelegateIvars {
    custom_protocol: crate::CustomProtocol,
}

#[cfg(feature = "custom_protocol")]
define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = CustomProtocolDelegateIvars]
    struct CustomProtocolDelegate;

    impl CustomProtocolDelegate {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn _start_url_scheme_task(&self, _: *mut Object, url_scheme_task: *mut Object) {
            self.start_url_scheme_task(url_scheme_task);
        }
    }
);

#[cfg(feature = "custom_protocol")]
impl CustomProtocolDelegate {
    fn start_url_scheme_task(&self, url_scheme_task: *mut Object) {
        let custom_protocol = &self.ivars().custom_protocol;
        let ns_request: *mut Object = unsafe { msg_send![url_scheme_task, request] };
        let req = ns_request_to_http_request(ns_request);
        let res = (custom_protocol.handler)(&req);
        let (ns_response, ns_data) = http_response_to_ns_response(&res, &req);
        unsafe {
            let _: () = msg_send![url_scheme_task, didReceiveResponse:ns_response.as_ptr()];
            let _: () = msg_send![url_scheme_task, didReceiveData:ns_data.as_ptr()];
            let _: () = msg_send![url_scheme_task, didFinish];
        }
    }
}

#[cfg(feature = "custom_protocol")]
fn ns_request_to_http_request(ns_request: *mut Object) -> small_http::Request {
    use std::str::FromStr;

    let method: NSString = unsafe { msg_send![ns_request, HTTPMethod] };
    let method = method.to_string();
    let url: *mut Object = unsafe { msg_send![ns_request, URL] };
    let url: NSString = unsafe { msg_send![url, absoluteString] };
    let url = url.to_string();
    let mut req = small_http::Request::with_method_and_url(
        small_http::Method::from_str(&method).unwrap_or(small_http::Method::Get),
        &url,
    );

    let headers: *mut Object = unsafe { msg_send![ns_request, allHTTPHeaderFields] };
    let keys: *mut Object = unsafe { msg_send![headers, allKeys] };
    let count: usize = unsafe { msg_send![keys, count] };
    for i in 0..count {
        let key: NSString = unsafe { msg_send![keys, objectAtIndex:i] };
        let value: NSString = unsafe { msg_send![headers, objectForKey:&*key] };
        req = req.header(key.to_string(), value.to_string());
    }

    let body: *mut Object = unsafe { msg_send![ns_request, HTTPBody] };
    if !body.is_null() {
        let length: usize = unsafe { msg_send![body, length] };
        let bytes: *const c_void = unsafe { msg_send![body, bytes] };
        let mut post_data = Vec::with_capacity(length);
        if length != 0 {
            assert!(
                !bytes.is_null(),
                "NSData returned null bytes for non-empty data"
            );
            post_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(bytes.cast::<u8>(), length)
            });
        }
        req = req.body(post_data)
    }
    req
}

#[cfg(feature = "custom_protocol")]
fn http_response_to_ns_response(
    res: &small_http::Response,
    req: &small_http::Request,
) -> (Retained<Object>, Retained<Object>) {
    let ns_response: Retained<Object> = unsafe {
        let url: *mut Object =
            msg_send![class!(NSURL), URLWithString:&*NSString::new(req.url.to_string())];

        let headers: *mut Object = msg_send![class!(NSMutableDictionary), dictionary];
        for (key, value) in &res.headers {
            let _: () =
                msg_send![headers, setObject:&*NSString::new(value), forKey:&*NSString::new(key)];
        }

        let ns_response: Allocated<Object> = msg_send![class!(NSHTTPURLResponse), alloc];
        let ns_response: Retained<Object> = msg_send![
            ns_response,
            initWithURL:url,
            statusCode:res.status as i64,
            HTTPVersion:&*NSString::new(req.version.to_string()),
            headerFields:headers
        ];
        ns_response
    };
    let ns_data: Retained<Object> = unsafe {
        msg_send![class!(NSData), dataWithBytes:res.body.as_ptr() as *const c_void, length:res.body.len()]
    };
    (ns_response, ns_data)
}
