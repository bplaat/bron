/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Browsing the images that sit next to the open one in its folder, like Finder lists them.

use std::cmp::Ordering;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use imageview_core::appkit::ns_string;
use objc2::rc::autoreleasepool;
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, msg_send};

/// Returns the path of the image `offset` places from `path` inside its folder.
pub(crate) fn neighbour_path(path: &Path, offset: isize) -> Option<PathBuf> {
    // The names compared are autoreleased.
    autoreleasepool(|_| {
        imageview_core::neighbour_path(path, offset, imageview_core::is_supported, compare_names)
    })
}

// Compares two file names the way Finder sorts them, which counts the numbers in a name.
fn compare_names(left: &str, right: &str) -> Ordering {
    // SAFETY: Both strings are autoreleased and valid for this call.
    let result: isize =
        unsafe { msg_send![ns_string(left), localizedStandardCompare: ns_string(right)] };
    result.cmp(&0)
}

/// Returns an autoreleased file `NSURL` for a path, or null for a path holding a null byte.
pub(crate) unsafe fn file_url(path: &Path) -> *mut Object {
    let Ok(representation) = CString::new(path.as_os_str().as_bytes()) else {
        return null_mut();
    };
    // SAFETY: The representation is a null terminated path that outlives this call, and the URL
    // is autoreleased.
    unsafe {
        msg_send![class!(NSURL),
            fileURLWithFileSystemRepresentation: representation.as_ptr(),
            isDirectory: Bool::NO,
            relativeToURL: null_mut::<Object>()
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder_with(name: &str, files: &[&str]) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("imageview-browse-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("create test folder");
        for file in files {
            std::fs::write(folder.join(file), b"").expect("create test file");
        }
        folder
    }

    fn next_name(path: &Path, offset: isize) -> Option<String> {
        neighbour_path(path, offset).map(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
    }

    #[test]
    fn a_folder_is_browsed_the_way_finder_sorts_it() {
        let folder = folder_with(
            "sorted",
            &[
                "10.png",
                "2.png",
                "notes.txt",
                ".hidden.png",
                "photo.jpeg",
                "photo.jfif",
            ],
        );
        // Numbers sort naturally; hidden and unreadable files are skipped.
        assert_eq!(
            next_name(&folder.join("2.png"), 1).as_deref(),
            Some("10.png")
        );
        assert_eq!(
            next_name(&folder.join("10.png"), 1).as_deref(),
            Some("photo.jfif")
        );
        assert_eq!(
            next_name(&folder.join("photo.jfif"), 1).as_deref(),
            Some("photo.jpeg")
        );
        assert_eq!(
            next_name(&folder.join("photo.jpeg"), 1).as_deref(),
            Some("2.png")
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_open_file_is_part_of_its_folder_whatever_it_holds() {
        let folder = folder_with("unknown", &["drawing.unknown", "photo.png"]);
        assert_eq!(
            next_name(&folder.join("drawing.unknown"), 1).as_deref(),
            Some("photo.png")
        );
        // An unopened file of an unreadable type is skipped.
        assert_eq!(next_name(&folder.join("photo.png"), 1), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
