/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;
use std::rc::{Rc, Weak};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use bwindow::ffi::*;

use crate::{InjectionTime, WebviewBuilder, WebviewEvent};

// Custom protocols are served from this domain, which Android reserves for local content.
const HOST_SUFFIX: &str = ".appassets.androidplatform.net";

// The Java WebView glue, compiled by the build script.
static DEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/classes.dex"));

// The nl.bplaat.bwebview.Bwebview class with its native methods registered.
fn bwebview_class(env: JniEnv) -> jclass {
    static CLASS: OnceLock<(DexClassLoader, GlobalRef)> = OnceLock::new();
    let (_, class) = CLASS.get_or_init(|| {
        let loader = DexClassLoader::new(DEX);
        let class = loader.find_class(env, "nl.bplaat.bwebview.Bwebview");
        unsafe {
            env.register_natives(
                class,
                &[
                    native_method(
                        c"nativeEvent",
                        c"(JLjava/lang/String;Ljava/lang/String;)V",
                        native_event as *mut c_void,
                    ),
                    native_method(
                        c"nativeRequest",
                        c"(J[Ljava/lang/String;)[B",
                        native_request as *mut c_void,
                    ),
                    native_method(c"nativeBack", c"()V", native_back as *mut c_void),
                ],
            );
            (loader, GlobalRef::from_local(env, class))
        }
    });
    class.as_raw()
}

// Custom protocols by webview id, requests arrive on a WebView background thread.
#[cfg(feature = "custom_protocol")]
static PROTOCOLS: std::sync::Mutex<Option<HashMap<u64, Vec<crate::CustomProtocol>>>> =
    std::sync::Mutex::new(None);

// Rust main thread state that the Java WebView callbacks find by id.
#[derive(Default)]
struct WebviewState {
    event_handler: Option<crate::EventHandler>,
    url: RefCell<Option<String>>,
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static WEBVIEWS: RefCell<HashMap<u64, Weak<WebviewState>>> = RefCell::new(HashMap::new());
}

pub(crate) struct PlatformWebview {
    id: u64,
    attachment: crate::WindowAttachment,
    closed: Rc<Cell<bool>>,
    state: Rc<WebviewState>,
    schemes: Vec<String>,
    java: Option<Rc<GlobalRef>>,
}

impl PlatformWebview {
    pub(crate) fn new(attachment: crate::WindowAttachment) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            attachment,
            closed: Rc::new(Cell::new(false)),
            state: Rc::default(),
            schemes: Vec::new(),
            java: None,
        }
    }

    pub(crate) fn init_webview(&mut self, builder: WebviewBuilder<'_>) {
        let Some(crate::NativeWindowHandle::Android(activity)) =
            (unsafe { self.attachment.native_handle() })
        else {
            panic!("invalid native window handle");
        };
        self.state = Rc::new(WebviewState {
            event_handler: builder.event_handler,
            ..WebviewState::default()
        });
        WEBVIEWS.with(|webviews| {
            webviews
                .borrow_mut()
                .insert(self.id, Rc::downgrade(&self.state))
        });

        let id = self.id;
        #[cfg(feature = "custom_protocol")]
        {
            self.schemes = builder
                .custom_protocols
                .iter()
                .map(|protocol| protocol.scheme.clone())
                .collect();
            PROTOCOLS
                .lock()
                .expect("custom protocols poisoned")
                .get_or_insert_default()
                .insert(id, builder.custom_protocols);
        }

        let env = JniEnv::current();
        let java = unsafe {
            let user_agent = env.new_string(&format!("bwebview/{}", env!("CARGO_PKG_VERSION")));
            let schemes =
                env.new_string_array(&self.schemes.iter().map(String::as_str).collect::<Vec<_>>());
            let class = bwebview_class(env);
            let constructor = env.method(
                class,
                c"<init>",
                c"(Landroid/app/Activity;JILjava/lang/String;[Ljava/lang/String;)V",
            );
            let object = env.new_object(
                class,
                constructor,
                &[
                    jvalue { l: activity },
                    jvalue { j: id as jlong },
                    // Transparent means no background color, others are opaque.
                    jvalue {
                        i: self.attachment.background_color().map_or(0, opaque),
                    },
                    jvalue { l: user_agent },
                    jvalue { l: schemes },
                ],
            );
            for local in [schemes, user_agent] {
                env.delete_local_ref(local);
            }
            Rc::new(GlobalRef::from_local(env, object))
        };
        self.java = Some(java.clone());

        crate::WebviewInterface::add_user_script(
            self,
            super::super::IPC_SCRIPT,
            InjectionTime::DocumentStart,
        );
        if let Some(url) = builder.should_load_url {
            crate::WebviewInterface::load_url(self, url);
        }
        if let Some(html) = builder.should_load_html {
            crate::WebviewInterface::load_html(self, html);
        }

        let closed = self.closed.clone();
        self.attachment.on_close(move || {
            if !closed.replace(true) {
                close(id, &java);
            }
        });
    }

    fn call(&self, name: &CStr, signature: &CStr, args: &[jvalue]) {
        if self.closed.get() {
            return;
        }
        let Some(java) = &self.java else {
            return;
        };
        unsafe { JniEnv::current().call_void_method(java.as_raw(), name, signature, args) };
    }

    fn call_with_string(&self, name: &CStr, value: &str) {
        let env = JniEnv::current();
        let string = env.new_string(value);
        self.call(name, c"(Ljava/lang/String;)V", &[jvalue { l: string }]);
        unsafe { env.delete_local_ref(string) };
    }
}

impl crate::WebviewInterface for PlatformWebview {
    fn url(&self) -> Option<String> {
        self.state.url.borrow().clone()
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        let url = rewrite_custom_protocol(url.as_ref(), &self.schemes);
        self.call_with_string(c"loadUrl", &url);
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        self.call_with_string(c"loadHtml", html.as_ref());
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        self.call_with_string(c"evaluateScript", script.as_ref());
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: InjectionTime) {
        let env = JniEnv::current();
        let script = env.new_string(script.as_ref());
        self.call(
            c"addUserScript",
            c"(Ljava/lang/String;Z)V",
            &[
                jvalue { l: script },
                jvalue {
                    z: (injection_time == InjectionTime::DocumentStart).into(),
                },
            ],
        );
        unsafe { env.delete_local_ref(script) };
    }

    fn set_background_color(&mut self, color: u32) {
        self.call(
            c"setBackgroundColor",
            c"(I)V",
            &[jvalue { i: opaque(color) }],
        );
    }
}

impl Drop for PlatformWebview {
    fn drop(&mut self) {
        self.attachment.disconnect();
        if !self.closed.replace(true)
            && let Some(java) = &self.java
        {
            close(self.id, java);
        }
    }
}

// Convert a 0xRRGGBB color to an opaque Android ARGB color.
const fn opaque(color: u32) -> jint {
    (color | 0xff00_0000) as jint
}

fn close(id: u64, java: &GlobalRef) {
    WEBVIEWS.with(|webviews| webviews.borrow_mut().remove(&id));
    #[cfg(feature = "custom_protocol")]
    if let Some(protocols) = PROTOCOLS
        .lock()
        .expect("custom protocols poisoned")
        .as_mut()
    {
        protocols.remove(&id);
    }
    unsafe { JniEnv::current().call_void_method(java.as_raw(), c"close", c"()V", &[]) };
}

// MARK: Native methods
extern "C" fn native_event(env: *mut JNIEnv, _: jclass, id: jlong, kind: jstring, value: jstring) {
    let env = unsafe { JniEnv::from_raw(env) };
    let (kind, value) = unsafe { (env.string(kind), env.string(value)) };
    android_post(move || dispatch_event(id as u64, &kind, &value));
}

#[cfg_attr(
    not(feature = "custom_protocol"),
    allow(clippy::missing_const_for_fn, reason = "a JNI entry point")
)]
extern "C" fn native_request(
    env: *mut JNIEnv,
    _: jclass,
    id: jlong,
    request: jobjectArray,
) -> jbyteArray {
    #[cfg(feature = "custom_protocol")]
    {
        let env = unsafe { JniEnv::from_raw(env) };
        let values = unsafe { env.string_array(request) };
        let protocols = PROTOCOLS.lock().expect("custom protocols poisoned");
        if let Some(response) = protocols
            .as_ref()
            .and_then(|protocols| protocols.get(&(id as u64)))
            .and_then(|protocols| handle_request(protocols, &values))
        {
            return env.new_byte_array(&response);
        }
    }
    #[cfg(not(feature = "custom_protocol"))]
    let _ = (env, id, request);
    null_mut()
}

extern "C" fn native_back(_: *mut JNIEnv, _: jclass) {
    android_back_pressed();
}

fn dispatch_event(id: u64, kind: &str, value: &str) {
    let Some(state) = WEBVIEWS.with(|webviews| webviews.borrow().get(&id).and_then(Weak::upgrade))
    else {
        return;
    };
    let event = match kind {
        "start" => {
            *state.url.borrow_mut() = Some(value.to_string());
            WebviewEvent::PageLoadStart
        }
        "finish" => WebviewEvent::PageLoadFinish,
        "title" => WebviewEvent::PageTitleChange(value.to_string()),
        // Messages are prefixed with 'i' for IPC or 'c' and a level char for the console.
        "message" => {
            #[cfg(feature = "log")]
            if let Some(message) = value.strip_prefix('c') {
                let mut message = message.chars();
                let level = message.next();
                let message = message.as_str();
                match level {
                    Some('e') => log::error!("{message}"),
                    Some('w') => log::warn!("{message}"),
                    Some('i' | 'l') => log::info!("{message}"),
                    Some('d') => log::debug!("{message}"),
                    Some('t') => log::trace!("{message}"),
                    _ => {}
                }
                return;
            }
            let Some(message) = value.strip_prefix('i') else {
                return;
            };
            WebviewEvent::MessageReceive(message.to_string())
        }
        _ => return,
    };
    if let Some(handler) = &state.event_handler {
        handler(event);
    }
}

// Map `scheme://path` to the https origin that shouldInterceptRequest serves.
fn rewrite_custom_protocol(url: &str, schemes: &[String]) -> String {
    for scheme in schemes {
        if let Some(rest) = url.strip_prefix(&format!("{scheme}://")) {
            return format!("https://{scheme}{HOST_SUFFIX}/{rest}");
        }
    }
    url.to_string()
}

// Serve a {method, url, header name, header value...} request, the response is a status line,
// header lines, an empty line, and the body.
#[cfg(feature = "custom_protocol")]
fn handle_request(protocols: &[crate::CustomProtocol], values: &[String]) -> Option<Vec<u8>> {
    use std::str::FromStr;

    let [method, url, headers @ ..] = values else {
        return None;
    };
    let mut request = small_http::Request::with_method_and_url(
        small_http::Method::from_str(method).unwrap_or(small_http::Method::Get),
        url,
    );
    for [name, value] in headers.as_chunks::<2>().0 {
        request = request.header(name.clone(), value.clone());
    }
    let host = request.url.host()?.to_string();
    let protocol = protocols
        .iter()
        .find(|protocol| host.strip_suffix(HOST_SUFFIX) == Some(protocol.scheme.as_str()))?;
    let response = (protocol.handler)(request);
    let mut bytes = format!("{}\n", response.status).into_bytes();
    // Line breaks in headers would break the line based response format.
    let line = |value: &str| value.replace(['\r', '\n'], " ");
    for (name, value) in &response.headers {
        bytes.extend_from_slice(format!("{}: {}\n", line(name), line(value)).as_bytes());
    }
    bytes.push(b'\n');
    bytes.reserve_exact(response.body.len());
    bytes.extend_from_slice(&response.body);
    Some(bytes)
}
