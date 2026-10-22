/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [directories](https://crates.io/crates/directories) crate

#![allow(unused)]

use std::path::PathBuf;

#[cfg(not(any(unix, windows)))]
compile_error!("Unsupported platform");

// MARK: Windows
#[cfg(windows)]
#[allow(unsafe_code)]
pub(crate) mod windows {
    use super::*;

    #[repr(C)]
    #[derive(PartialEq, Eq)]
    pub(crate) struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(pv: *mut std::ffi::c_void);
    }

    #[allow(clippy::upper_case_acronyms)]
    pub(crate) struct LPWSTR(*mut u16);
    impl Default for LPWSTR {
        fn default() -> Self {
            Self(std::ptr::null_mut())
        }
    }
    impl LPWSTR {
        pub(crate) const fn as_mut_ptr(&mut self) -> *mut *mut u16 {
            &mut self.0
        }
    }
    impl std::fmt::Display for LPWSTR {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            if self.0.is_null() {
                return Ok(());
            }
            let mut len = 0;
            // SAFETY: self.0 is a valid non-null Windows wide string pointer (checked above); scanning until the null terminator is safe.
            unsafe {
                while *self.0.add(len) != 0 {
                    len += 1;
                }
            }
            // SAFETY: self.0 is non-null and len is the number of valid u16 code units before the null terminator.
            let str = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(self.0, len) });
            write!(f, "{str}")
        }
    }
    impl Drop for LPWSTR {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: self.0 is a non-null pointer previously allocated by SHGetKnownFolderPath via COM and not yet freed.
                unsafe { CoTaskMemFree(self.0 as *mut std::ffi::c_void) };
            }
        }
    }

    const KF_FLAG_DEFAULT: u32 = 0x00000000;
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetKnownFolderPath(
            rfid: *const Guid,
            dwFlags: u32,
            hToken: *const std::ffi::c_void,
            ppszPath: *mut *mut u16,
        ) -> i32;
    }

    pub(crate) const FOLDERID_LOCAL_APPDATA: Guid = Guid {
        data1: 0xF1B32785,
        data2: 0x6FBA,
        data3: 0x4FCF,
        data4: [0x9D, 0x55, 0x7B, 0x8E, 0x7F, 0x15, 0x70, 0x91],
    };
    pub(crate) const FOLDERID_ROAMING_APPDATA: Guid = Guid {
        data1: 0x3EB685DB,
        data2: 0x65F9,
        data3: 0x4CF6,
        data4: [0xA0, 0x3A, 0xE3, 0xEF, 0x65, 0x72, 0x9F, 0x3D],
    };
    pub(crate) const FOLDERID_DESKTOP: Guid = Guid {
        data1: 0xB4BFCC3A,
        data2: 0xDB2C,
        data3: 0x424C,
        data4: [0xB0, 0x29, 0x7F, 0xE9, 0x9A, 0x87, 0xC6, 0x41],
    };
    pub(crate) const FOLDERID_MUSIC: Guid = Guid {
        data1: 0x4BD8D571,
        data2: 0x6D19,
        data3: 0x48D3,
        data4: [0xBE, 0x97, 0x42, 0x22, 0x20, 0x08, 0x0E, 0x43],
    };

    pub(crate) fn get_known_folder_path(folder_id: &Guid) -> PathBuf {
        let mut path = LPWSTR::default();
        // SAFETY: folder_id is a valid GUID reference; path.as_mut_ptr() provides a valid output pointer; null token means current user.
        unsafe {
            SHGetKnownFolderPath(
                folder_id,
                KF_FLAG_DEFAULT,
                std::ptr::null(),
                path.as_mut_ptr(),
            )
        };
        PathBuf::from(path.to_string())
    }

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_IS_APP_CONTAINER: u32 = 29;
    const ERROR_SUCCESS: i32 = 0;
    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
        fn GetCurrentPackageFamilyName(length: *mut u32, name: *mut u16) -> i32;
    }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(
            process: *mut std::ffi::c_void,
            access: u32,
            token: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn GetTokenInformation(
            token: *mut std::ffi::c_void,
            class: u32,
            information: *mut std::ffi::c_void,
            length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    fn is_app_container() -> bool {
        let mut token = std::ptr::null_mut();
        // SAFETY: GetCurrentProcess returns a pseudo-handle and token points to writable storage.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return false;
        }
        let mut is_app_container = 0u32;
        let mut length = 0;
        // SAFETY: token is a valid token handle and is_app_container is a writable u32 buffer.
        let success = unsafe {
            GetTokenInformation(
                token,
                TOKEN_IS_APP_CONTAINER,
                (&raw mut is_app_container).cast(),
                size_of::<u32>() as u32,
                &mut length,
            )
        } != 0;
        // SAFETY: token is an owned handle returned by OpenProcessToken and is closed once.
        unsafe { CloseHandle(token) };
        success && is_app_container != 0
    }

    fn package_family_name() -> Option<String> {
        let mut length = 0;
        // SAFETY: Querying the required length with a null buffer is allowed.
        if unsafe { GetCurrentPackageFamilyName(&mut length, std::ptr::null_mut()) }
            != ERROR_INSUFFICIENT_BUFFER
        {
            return None;
        }
        let mut name = vec![0u16; length as usize];
        // SAFETY: name has room for length code units including the null terminator.
        if unsafe { GetCurrentPackageFamilyName(&mut length, name.as_mut_ptr()) } != ERROR_SUCCESS {
            return None;
        }
        Some(String::from_utf16_lossy(
            &name[..length.saturating_sub(1) as usize],
        ))
    }

    /// Returns the AppData folder, sandboxed MSIX apps can only write in their own package folders
    pub(crate) fn get_app_data_path(folder_id: &Guid) -> PathBuf {
        static PACKAGE_DIR: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
        let package_dir = PACKAGE_DIR.get_or_init(|| {
            if !is_app_container() {
                return None;
            }
            package_family_name().map(|name| {
                // Inside an AppContainer the local AppData folder already points to the
                // package's AC folder
                let local = get_known_folder_path(&FOLDERID_LOCAL_APPDATA);
                let package_dir = std::path::Path::new("Packages").join(name);
                match local.parent() {
                    Some(parent) if local.ends_with(package_dir.join("AC")) => parent.to_owned(),
                    _ => local.join(package_dir),
                }
            })
        });
        match package_dir {
            Some(package_dir) if *folder_id == FOLDERID_ROAMING_APPDATA => {
                package_dir.join("RoamingState")
            }
            Some(package_dir) => package_dir.join("LocalState"),
            None => get_known_folder_path(folder_id),
        }
    }
}

// MARK: Android
#[cfg(target_os = "android")]
#[allow(unsafe_code)]
pub(crate) mod android {
    use super::*;

    unsafe extern "C" {
        fn getuid() -> u32;
    }

    // Every Android user profile has its own range of app user ids
    const PER_USER_RANGE: u32 = 100_000;

    /// Returns the private data directory of the app, like Context.getDataDir()
    pub(crate) fn data_dir() -> Option<PathBuf> {
        let cmdline = std::fs::read("/proc/self/cmdline").ok()?;
        // The process name is the package name, other app processes add a :name suffix
        let name = cmdline.split(|&byte| byte == 0 || byte == b':').next()?;
        let package = std::str::from_utf8(name)
            .ok()
            .filter(|name| !name.is_empty())?;
        // SAFETY: getuid has no preconditions and always succeeds.
        let user = unsafe { getuid() } / PER_USER_RANGE;
        Some(PathBuf::from(format!("/data/user/{user}/{package}")))
    }
}

#[cfg(all(unix, not(target_vendor = "apple"), not(target_os = "android")))]
fn xdg_dir(name: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or(fallback)
}

// MARK: BaseDirs
/// Provides paths of user-invisible standard directories
pub struct BaseDirs {
    home_dir: PathBuf,
}

impl BaseDirs {
    /// Creates a BaseDirs struct which holds the paths to user-invisible directories
    pub fn new() -> Option<Self> {
        Some(Self {
            home_dir: cfg_select! {
                target_os = "android" => android::data_dir()?,
                _ => std::env::home_dir()?,
            },
        })
    }

    /// Returns the path to the user's home directory, not available on sandboxed Android and iOS
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn home_dir(&self) -> PathBuf {
        self.home_dir.clone()
    }

    /// Returns the path to the user's cache directory
    pub fn cache_dir(&self) -> PathBuf {
        cfg_select! {
            target_os = "android" => self.home_dir.join("cache"),
            all(unix, not(target_vendor = "apple")) => {
                xdg_dir("XDG_CACHE_HOME", self.home_dir.join(".cache"))
            }
            target_vendor = "apple" => self.home_dir.join("Library").join("Caches"),
            windows => windows::get_app_data_path(&windows::FOLDERID_LOCAL_APPDATA),
            _ => unreachable!(),
        }
    }

    /// Returns the path to the user's config directory
    pub fn config_dir(&self) -> PathBuf {
        cfg_select! {
            target_os = "android" => self.home_dir.join("files"),
            all(unix, not(target_vendor = "apple")) => {
                xdg_dir("XDG_CONFIG_HOME", self.home_dir.join(".config"))
            }
            target_vendor = "apple" => self.home_dir.join("Library").join("Application Support"),
            windows => windows::get_app_data_path(&windows::FOLDERID_ROAMING_APPDATA),
            _ => unreachable!(),
        }
    }

    /// Returns the path to the user's data directory
    pub fn data_dir(&self) -> PathBuf {
        cfg_select! {
            target_os = "android" => self.home_dir.join("files"),
            all(unix, not(target_vendor = "apple")) => {
                xdg_dir("XDG_DATA_HOME", self.home_dir.join(".local").join("share"))
            }
            target_vendor = "apple" => self.home_dir.join("Library").join("Application Support"),
            windows => windows::get_app_data_path(&windows::FOLDERID_ROAMING_APPDATA),
            _ => unreachable!(),
        }
    }

    /// Returns the path to the user's local data directory
    pub fn data_local_dir(&self) -> PathBuf {
        cfg_select! {
            windows => windows::get_app_data_path(&windows::FOLDERID_LOCAL_APPDATA),
            _ => self.data_dir(),
        }
    }

    /// Returns the path to the user's executable directory, only available on Linux and BSD,
    /// not available on sandboxed Android and iOS
    // Const-compatible on macOS and Windows, but reads the environment on Linux.
    #[allow(clippy::missing_const_for_fn)]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn executable_dir(&self) -> Option<PathBuf> {
        cfg_select! {
            all(unix, not(target_vendor = "apple")) => Some(xdg_dir(
                "XDG_BIN_HOME",
                self.home_dir.join(".local").join("bin"),
            )),
            _ => None,
        }
    }
}

// MARK: ProjectDirs
/// Computes the location of cache, config or data directories for a specific application
pub struct ProjectDirs {
    base_dirs: BaseDirs,
    project_path: PathBuf,
}

impl ProjectDirs {
    /// Creates a ProjectDirs struct from values describing the project
    pub fn from(qualifier: &str, organization: &str, application: &str) -> Option<Self> {
        Self::from_path(cfg_select! {
            // Mobile apps are sandboxed, so their directories already belong to the app
            any(target_os = "android", target_os = "ios") => {
                PathBuf::new()
            }
            all(unix, not(target_vendor = "apple")) => {
                PathBuf::from(application)
            }
            target_vendor = "apple" => {
                PathBuf::from(format!("{qualifier}.{organization}.{application}"))
            }
            windows => PathBuf::from(organization).join(application),
            _ => unreachable!(),
        })
    }

    /// Creates a ProjectDirs struct from a project path. This is strongly discouraged, as its results will not follow operating system standards.
    pub fn from_path(project_path: PathBuf) -> Option<Self> {
        Some(Self {
            base_dirs: BaseDirs::new()?,
            project_path,
        })
    }

    /// Returns the path to the project's cache directory
    pub fn cache_dir(&self) -> PathBuf {
        self.base_dirs.cache_dir().join(&self.project_path)
    }

    /// Returns the path to the project's config directory
    pub fn config_dir(&self) -> PathBuf {
        self.base_dirs.config_dir().join(&self.project_path)
    }
}

// MARK: UserDirs
/// Provides paths of user-facing standard directories, not available on sandboxed Android and iOS
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub struct UserDirs {
    home_dir: PathBuf,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl UserDirs {
    /// Creates a UserDirs struct which holds the paths to user-facing directories
    pub fn new() -> Option<Self> {
        let home_dir = std::env::home_dir()?;
        Some(Self { home_dir })
    }

    /// Returns the path to the user's desktop directory
    pub fn desktop_dir(&self) -> PathBuf {
        cfg_select! {
            windows => windows::get_known_folder_path(&windows::FOLDERID_DESKTOP),
            _ => self.home_dir.join("Desktop"),
        }
    }

    /// Returns the path to the user's audio directory
    pub fn audio_dir(&self) -> PathBuf {
        cfg_select! {
            all(unix, not(target_vendor = "apple")) => {
                let xdg_music = std::env::var("XDG_MUSIC_DIR").map(PathBuf::from);
                xdg_music.unwrap_or_else(|_| self.home_dir.join("Music"))
            }
            target_vendor = "apple" => self.home_dir.join("Music"),
            windows => windows::get_known_folder_path(&windows::FOLDERID_MUSIC),
            _ => unreachable!(),
        }
    }
}
