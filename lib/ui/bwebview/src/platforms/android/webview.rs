/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::{Rc, Weak};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use bwindow::ffi::{android_back_pressed, android_post, android_with_env};
use jni::errors::{Result, ThrowRuntimeExAndDefault};
use jni::objects::{Global, JByteArray, JClass, JObject, JObjectArray, JString};
use jni::strings::JNIStr;
use jni::sys::{jint, jlong};
use jni::{Env, EnvUnowned, JValue, NativeMethod, jni_sig, jni_str};

use crate::{InjectionTime, WebviewBuilder, WebviewEvent};

// Custom protocols are served from this domain, which Android reserves for local content.
const HOST_SUFFIX: &str = ".appassets.androidplatform.net";

// The Java WebView glue, compiled by the build script.
static DEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/classes.dex"));

// The nl.bplaat.bwebview.Bwebview class with its native methods registered, loaded from the
// dex code with the class loader of the activity as parent, the class keeps its loader alive.
fn bwebview_class(env: &mut Env, activity: &JObject) -> Result<&'static Global<JClass<'static>>> {
    static CLASS: OnceLock<Global<JClass<'static>>> = OnceLock::new();
    if let Some(class) = CLASS.get() {
        return Ok(class);
    }
    let parent = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let dex = unsafe { env.new_direct_byte_buffer(DEX.as_ptr().cast_mut(), DEX.len())? };
    let loader_class = env.find_class(jni_str!("dalvik/system/InMemoryDexClassLoader"))?;
    let loader = env.new_object(
        &loader_class,
        jni_sig!("(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V"),
        &[(&dex).into(), (&parent).into()],
    )?;
    let name = env.new_string("nl.bplaat.bwebview.Bwebview")?;
    let class = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[(&name).into()],
        )?
        .l()?;
    let class = unsafe { JClass::from_raw(env, class.into_raw()) };
    unsafe {
        env.register_native_methods(
            &class,
            &[
                NativeMethod::from_raw_parts(
                    jni_str!("nativeEvent"),
                    jni_str!("(JLjava/lang/String;Ljava/lang/String;)V"),
                    native_event as *mut c_void,
                ),
                NativeMethod::from_raw_parts(
                    jni_str!("nativeRequest"),
                    jni_str!("(J[Ljava/lang/String;)[B"),
                    native_request as *mut c_void,
                ),
                NativeMethod::from_raw_parts(
                    jni_str!("nativeBack"),
                    jni_str!("()V"),
                    native_back as *mut c_void,
                ),
            ],
        )?;
    }
    let class = env.new_global_ref(&class)?;
    Ok(CLASS.get_or_init(|| class))
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
    java: Option<Rc<Global<JObject<'static>>>>,
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

        // Transparent means no background color, others are opaque.
        let background = self.attachment.background_color().map_or(0, opaque);
        let java = android_with_env(|env| {
            let activity = unsafe { JObject::from_raw(env, activity.cast()) };
            let class = bwebview_class(env, &activity)?;
            let user_agent = env.new_string(format!("bwebview/{}", env!("CARGO_PKG_VERSION")))?;
            let schemes = JObjectArray::<JString>::new(env, self.schemes.len(), JString::null())?;
            for (index, scheme) in self.schemes.iter().enumerate() {
                let scheme = env.new_string(scheme)?;
                schemes.set_element(env, index, &scheme)?;
            }
            let object = env.new_object(
                class,
                jni_sig!("(Landroid/app/Activity;JILjava/lang/String;[Ljava/lang/String;)V"),
                &[
                    (&activity).into(),
                    JValue::Long(id as jlong),
                    JValue::Int(background),
                    (&user_agent).into(),
                    (&schemes).into(),
                ],
            )?;
            Ok(Rc::new(env.new_global_ref(&object)?))
        });
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

    fn call(&self, f: impl FnOnce(&mut Env, &JObject<'static>) -> Result<()>) {
        if self.closed.get() {
            return;
        }
        if let Some(java) = &self.java {
            android_with_env(|env| f(env, java.as_obj()));
        }
    }

    fn call_with_string(&self, name: &'static JNIStr, value: &str) {
        self.call(|env, java| {
            let value = env.new_string(value)?;
            env.call_method(
                java,
                name,
                jni_sig!("(Ljava/lang/String;)V"),
                &[(&value).into()],
            )?
            .v()
        });
    }
}

impl crate::WebviewInterface for PlatformWebview {
    fn url(&self) -> Option<String> {
        self.state.url.borrow().clone()
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        let url = rewrite_custom_protocol(url.as_ref(), &self.schemes);
        self.call_with_string(jni_str!("loadUrl"), &url);
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        self.call_with_string(jni_str!("loadHtml"), html.as_ref());
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        self.call_with_string(jni_str!("evaluateScript"), script.as_ref());
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: InjectionTime) {
        self.call(|env, java| {
            let script = env.new_string(script)?;
            env.call_method(
                java,
                jni_str!("addUserScript"),
                jni_sig!("(Ljava/lang/String;Z)V"),
                &[
                    (&script).into(),
                    JValue::Bool(injection_time == InjectionTime::DocumentStart),
                ],
            )?
            .v()
        });
    }

    fn set_background_color(&mut self, color: u32) {
        self.call(|env, java| {
            env.call_method(
                java,
                jni_str!("setBackgroundColor"),
                jni_sig!("(I)V"),
                &[JValue::Int(opaque(color))],
            )?
            .v()
        });
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

fn close(id: u64, java: &Global<JObject<'static>>) {
    WEBVIEWS.with(|webviews| webviews.borrow_mut().remove(&id));
    #[cfg(feature = "custom_protocol")]
    if let Some(protocols) = PROTOCOLS
        .lock()
        .expect("custom protocols poisoned")
        .as_mut()
    {
        protocols.remove(&id);
    }
    android_with_env(|env| {
        env.call_method(java.as_obj(), jni_str!("close"), jni_sig!("()V"), &[])?
            .v()
    });
}

// MARK: Native methods
// Read a Java string, null reads as an empty string.
fn string(env: &Env, string: &JString) -> Result<String> {
    if string.is_null() {
        Ok(String::new())
    } else {
        string.try_to_string(env)
    }
}

extern "system" fn native_event<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    id: jlong,
    kind: JString<'local>,
    value: JString<'local>,
) {
    env.with_env(|env| -> Result<()> {
        let (kind, value) = (string(env, &kind)?, string(env, &value)?);
        android_post(move || dispatch_event(id as u64, &kind, &value));
        Ok(())
    })
    .resolve::<ThrowRuntimeExAndDefault>();
}

extern "system" fn native_request<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    id: jlong,
    request: JObjectArray<'local, JString<'local>>,
) -> JByteArray<'local> {
    env.with_env(|env| response(env, id as u64, &request))
        .resolve::<ThrowRuntimeExAndDefault>()
}

#[cfg(feature = "custom_protocol")]
fn response<'local>(
    env: &mut Env<'local>,
    id: u64,
    request: &JObjectArray<JString>,
) -> Result<JByteArray<'local>> {
    let mut values = Vec::new();
    for index in 0..request.len(env)? {
        let value = request.get_element(env, index)?;
        values.push(string(env, &value)?);
    }
    let protocols = PROTOCOLS.lock().expect("custom protocols poisoned");
    match protocols
        .as_ref()
        .and_then(|protocols| protocols.get(&id))
        .and_then(|protocols| handle_request(protocols, &values))
    {
        Some(response) => env.byte_array_from_slice(&response),
        None => Ok(JByteArray::default()),
    }
}

#[cfg(not(feature = "custom_protocol"))]
fn response<'local>(
    _: &mut Env<'local>,
    _: u64,
    _: &JObjectArray<JString>,
) -> Result<JByteArray<'local>> {
    Ok(JByteArray::default())
}

extern "system" fn native_back<'local>(_: EnvUnowned<'local>, _: JClass<'local>) {
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
    let response = (protocol.handler)(&request);
    let mut bytes = format!("{}\n", response.status).into_bytes();
    // Line breaks in headers would break the line based response format.
    let line = |value: &str| value.replace(['\r', '\n'], " ");
    for (name, value) in &response.headers {
        bytes.extend_from_slice(format!("{}: {}\n", line(name), line(value)).as_bytes());
    }
    bytes.push(b'\n');
    bytes.extend_from_slice(&response.body);
    Some(bytes)
}
