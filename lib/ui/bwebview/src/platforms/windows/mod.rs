/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(not(target_vendor = "winxp"))]
pub(crate) use self::webview2::PlatformWebview;

mod com;
#[cfg(feature = "file_drop")]
mod file_drop;
mod headers;
#[cfg(target_vendor = "winxp")]
mod miniblink;
mod webview2;

/// The `winxp` target uses WebView2 when its runtime is installed and MiniBlink otherwise, so
/// the same executable gets the modern browser on newer Windows.
#[cfg(target_vendor = "winxp")]
pub(crate) enum PlatformWebview {
    WebView2(webview2::PlatformWebview),
    MiniBlink(miniblink::PlatformWebview),
}

#[cfg(target_vendor = "winxp")]
macro_rules! forward {
    ($self:ident.$name:ident($($arg:expr),*)) => {
        match $self {
            Self::WebView2(webview) => webview.$name($($arg),*),
            Self::MiniBlink(webview) => webview.$name($($arg),*),
        }
    };
}

#[cfg(target_vendor = "winxp")]
impl PlatformWebview {
    pub(crate) fn new(attachment: bwindow::WindowAttachment) -> Self {
        if webview2::runtime_available() {
            Self::WebView2(webview2::PlatformWebview::new(attachment))
        } else {
            Self::MiniBlink(miniblink::PlatformWebview::new(attachment))
        }
    }

    pub(crate) fn init_webview(&mut self, builder: crate::WebviewBuilder<'_>) {
        forward!(self.init_webview(builder))
    }
}

#[cfg(target_vendor = "winxp")]
impl crate::WebviewInterface for PlatformWebview {
    fn url(&self) -> Option<String> {
        forward!(self.url())
    }

    fn load_url(&mut self, url: impl AsRef<str>) {
        forward!(self.load_url(url))
    }

    fn load_html(&mut self, html: impl AsRef<str>) {
        forward!(self.load_html(html))
    }

    fn evaluate_script(&mut self, script: impl AsRef<str>) {
        forward!(self.evaluate_script(script))
    }

    fn add_user_script(&mut self, script: impl AsRef<str>, injection_time: crate::InjectionTime) {
        forward!(self.add_user_script(script, injection_time))
    }

    fn set_background_color(&mut self, color: u32) {
        forward!(self.set_background_color(color))
    }
}
