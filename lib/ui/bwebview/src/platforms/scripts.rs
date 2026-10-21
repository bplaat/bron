/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

macro_rules! ipc_script {
    () => {
        r#"try { window.ipc = new EventTarget(); } catch (e) {
  window.ipc = { _h: {},
    addEventListener(t, l) { (this._h[t] = this._h[t] || []).push(l); },
    removeEventListener(t, l) { this._h[t] = (this._h[t] || []).filter(x => x !== l); },
    dispatchEvent(e) { (this._h[e.type] || []).forEach(l => l(e)); return true; }
  };
}
if (window.webkit) {
  window.ipc.postMessage = m =>
    window.webkit.messageHandlers.ipc.postMessage(typeof m !== 'string' ? JSON.stringify(m) : m);
} else {
  window.ipc.postMessage = m =>
    (window.bwebview || window.chrome.webview).postMessage('i' + (typeof m !== 'string' ? JSON.stringify(m) : m));
}
"#
    };
}

#[cfg(feature = "log")]
macro_rules! console_script {
    () => {
        r#"for (const level of ['error', 'warn', 'info', 'debug', 'trace', 'log']) {
  window.console[level] = (...args) => {
    const msg = args.map(arg => typeof arg !== 'string' ? JSON.stringify(arg) : arg).join(' ');
    if (window.webkit) {
      window.webkit.messageHandlers.console.postMessage(level.charAt(0) + msg);
    } else {
      (window.bwebview || window.chrome.webview).postMessage('c' + level.charAt(0) + msg);
    }
  };
}
"#
    };
}

cfg_select! {
    feature = "log" => {
        pub(crate) const IPC_SCRIPT: &str = concat!(ipc_script!(), "\n", console_script!());
    }
    _ => {
        pub(crate) const IPC_SCRIPT: &str = ipc_script!();
    }
}
