/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Minimal WebView2 Evergreen Runtime loader.
//!
//! This intentionally supports only normal stable Evergreen installations. It does not reproduce
//! WebView2Loader's policy, environment-variable, preview-channel, or packaged-app overrides.

use std::ffi::{OsStr, OsString, c_void};
use std::path::PathBuf;
use std::ptr::null_mut;

use super::headers::{
    ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler, ICoreWebView2EnvironmentOptions, *,
};

const STABLE_RUNTIME_KEY: &[u16] =
    w!("SOFTWARE\\Microsoft\\EdgeUpdate\\ClientState\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}");

// ICoreWebView2Settings2 is used unconditionally and first shipped in SDK 1.0.864.35.
// Newer interfaces used by bwebview are queried conditionally and have fallbacks.
const MIN_RUNTIME_API_VERSION: u32 = 864;

#[cfg(target_arch = "x86_64")]
const RUNTIME_ARCH: &str = "x64";
#[cfg(target_arch = "x86")]
const RUNTIME_ARCH: &str = "x86";
#[cfg(target_arch = "aarch64")]
const RUNTIME_ARCH: &str = "arm64";

type CreateEnvironment = unsafe extern "system" fn(
    bool,
    i32,
    *const u16,
    *mut ICoreWebView2EnvironmentOptions,
    *mut ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
) -> HRESULT;

const INSTALLED_RUNTIME: i32 = 0;

pub(super) const WEBVIEW2_RUNTIME_NOT_FOUND: HRESULT = hresult_from_win32(ERROR_FILE_NOT_FOUND);

pub(super) unsafe fn create_core_webview2_environment(
    user_data_folder: *const u16,
    environment_created_handler: *mut ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
) -> HRESULT {
    if environment_created_handler.is_null() {
        return E_POINTER;
    }

    let mut last_error = ERROR_FILE_NOT_FOUND;
    let mut loaded = None;
    for runtime_dir in find_evergreen_runtimes() {
        match load_runtime(runtime_dir) {
            Ok(runtime) => {
                loaded = Some(runtime);
                break;
            }
            Err(error) => last_error = error,
        }
    }
    let Some((_module, create)) = loaded else {
        return hresult_from_win32(last_error);
    };

    // Keep the module loaded because environment creation completes asynchronously.
    unsafe {
        create(
            true,
            INSTALLED_RUNTIME,
            user_data_folder,
            null_mut(),
            environment_created_handler,
        )
    }
}

fn load_runtime(runtime_dir: OsString) -> Result<(HMODULE, CreateEnvironment), u32> {
    let dll_path = wide(
        &PathBuf::from(runtime_dir)
            .join("EBWebView")
            .join(RUNTIME_ARCH)
            .join("EmbeddedBrowserWebView.dll")
            .to_string_lossy(),
    );

    let module = unsafe { LoadLibraryW(dll_path.as_ptr()) };
    if module.is_null() {
        return Err(unsafe { GetLastError() });
    }

    let create = unsafe {
        GetProcAddress(
            module,
            c"CreateWebViewEnvironmentWithOptionsInternal".as_ptr(),
        )
    };
    if create.is_null() {
        unsafe { FreeLibrary(module) };
        return Err(ERROR_PROC_NOT_FOUND);
    }

    Ok((module, unsafe {
        std::mem::transmute::<*const c_void, CreateEnvironment>(create)
    }))
}

/// Whether a supported WebView2 runtime is installed
#[cfg(target_vendor = "winxp")]
pub(crate) fn runtime_available() -> bool {
    !find_evergreen_runtimes().is_empty()
}

fn find_evergreen_runtimes() -> Vec<OsString> {
    [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
        .into_iter()
        .filter_map(read_runtime_dir)
        .filter(|runtime| {
            runtime_api_version(runtime).is_some_and(|api| api >= MIN_RUNTIME_API_VERSION)
        })
        .collect()
}

/// Reads the runtime folder from the 32-bit registry view, where EdgeUpdate registers it. Uses
/// `RegOpenKeyExW` because `RegGetValueW` only accepts `RRF_SUBKEY_WOW6432KEY` from Windows 10.
fn read_runtime_dir(root: HKEY) -> Option<OsString> {
    let mut key = null_mut();
    let status = unsafe {
        RegOpenKeyExW(
            root,
            STABLE_RUNTIME_KEY.as_ptr(),
            0,
            KEY_QUERY_VALUE | KEY_WOW64_32KEY,
            &mut key,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    let mut path = [0_u16; 512];
    let mut value_type = 0;
    let mut byte_len = size_of_val(&path) as u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            w!("EBWebView").as_ptr(),
            null_mut(),
            &mut value_type,
            path.as_mut_ptr().cast(),
            &mut byte_len,
        )
    };
    unsafe { RegCloseKey(key) };
    if status != ERROR_SUCCESS || value_type != REG_SZ {
        return None;
    }

    // The stored string isn't always null-terminated
    let path = &path[..byte_len as usize / 2];
    let len = path
        .iter()
        .position(|&value| value == 0)
        .unwrap_or(path.len());
    (len > 0).then(|| OsString::from(String::from_utf16_lossy(&path[..len])))
}

fn runtime_api_version(runtime_dir: &OsStr) -> Option<u32> {
    PathBuf::from(runtime_dir)
        .file_name()?
        .to_str()?
        .split('.')
        .nth(2)?
        .parse()
        .ok()
}

const fn hresult_from_win32(error: u32) -> HRESULT {
    if error == 0 {
        S_OK
    } else {
        (error & 0xffff | 0x8007_0000) as HRESULT
    }
}

const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PROC_NOT_FOUND: u32 = 127;

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_api_version() {
        assert_eq!(
            runtime_api_version(OsStr::new(
                r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\140.0.3485.54"
            )),
            Some(3485)
        );
    }

    #[test]
    fn accepts_last_windows_7_runtime() {
        let version = runtime_api_version(OsStr::new(
            r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application\109.0.1518.78",
        ));
        assert!(version.is_some_and(|api| api >= MIN_RUNTIME_API_VERSION));
    }

    #[test]
    fn rejects_invalid_runtime_version() {
        assert_eq!(
            runtime_api_version(OsStr::new(r"C:\WebView2\invalid")),
            None
        );
        assert_eq!(
            runtime_api_version(OsStr::new(r"C:\WebView2\1.0.beta.1")),
            None
        );
    }
}
