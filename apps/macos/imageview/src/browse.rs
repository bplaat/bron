/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Browsing the images that sit next to the open one in its folder.

use std::cmp::Ordering;
use std::ffi::{CStr, CString, OsStr, c_char};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use imageview_appkit::ns_string;
use objc2::runtime::{AnyObject as Object, Bool};
use objc2::{class, msg_send};

/// Returns the wrapped browsing index, or `None` when there is nothing to browse.
pub(crate) fn sibling_index(count: usize, current: usize, offset: isize) -> Option<usize> {
    if count == 0 || current >= count {
        return None;
    }
    let index = (current as isize + offset).rem_euclid(count as isize) as usize;
    (index != current).then_some(index)
}

/// Returns the path of the image `offset` places from `path` inside its folder.
///
/// # Safety
///
/// `document_class` must be an `NSDocument` subclass.
pub(crate) unsafe fn neighbour_path(
    path: &Path,
    offset: isize,
    document_class: *mut Object,
) -> Option<PathBuf> {
    // SAFETY: The caller supplies a document class whose readable types are process-lifetime
    // objects.
    unsafe {
        let siblings = openable_siblings(path, &readable_types(document_class));
        let current = siblings.iter().position(|sibling| sibling == path)?;
        sibling_index(siblings.len(), current, offset).map(|index| siblings[index].clone())
    }
}

// Returns Finder-sorted readable siblings, always including the open `path`.
//
// # Safety
//
// `readable` must point to valid `UTType` objects for the duration of this call.
unsafe fn openable_siblings(path: &Path, readable: &[*mut Object]) -> Vec<PathBuf> {
    let Some(folder) = path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };

    let mut siblings = Vec::new();
    for entry in entries.flatten() {
        let sibling = entry.path();
        let file = !is_hidden(&sibling) && entry.file_type().is_ok_and(|kind| !kind.is_dir());
        // SAFETY: The caller supplies valid types.
        let openable = file && unsafe { is_openable(&sibling, readable) };
        if openable || sibling == path {
            // SAFETY: The name is autoreleased and outlives the sorting below.
            siblings.push((unsafe { name_string(&sibling) }, sibling));
        }
    }
    // SAFETY: Every name is a valid string for as long as this function runs.
    siblings.sort_by(|(left, _), (right, _)| unsafe { compare_names(*left, *right) });
    siblings.into_iter().map(|(_, sibling)| sibling).collect()
}

// Returns whether the file's declared type conforms to one of the readable types.
//
// # Safety
//
// `readable` must point to valid `UTType` objects for the duration of this call.
unsafe fn is_openable(path: &Path, readable: &[*mut Object]) -> bool {
    let Some(extension) = path.extension().and_then(OsStr::to_str) else {
        return false;
    };
    // SAFETY: The type database answers with an autoreleased type or with nothing at all.
    unsafe {
        // macOS can classify .jfif as a dynamic type that does not conform to public.image.
        let kind: *mut Object = if extension.eq_ignore_ascii_case("jfif") {
            msg_send![class!(UTType), typeWithIdentifier: ns_string("public.jpeg")]
        } else {
            msg_send![class!(UTType), typeWithFilenameExtension: ns_string(extension)]
        };
        if kind.is_null() {
            return false;
        }
        readable.iter().any(|&readable| {
            let conforms: Bool = msg_send![kind, conformsToType: readable];
            conforms.as_bool()
        })
    }
}

// Returns the document class's bundle-declared readable types as `UTType` objects.
//
// # Safety
//
// `document_class` must be an `NSDocument` subclass.
unsafe fn readable_types(document_class: *mut Object) -> Vec<*mut Object> {
    // SAFETY: The caller supplies a document class, whose autoreleased type identifiers outlive
    // this call along with the types they are looked up as.
    unsafe {
        let identifiers: *mut Object = msg_send![document_class, readableTypes];
        if identifiers.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![identifiers, count];
        let mut types = Vec::with_capacity(count);
        for index in 0..count {
            let identifier: *mut Object = msg_send![identifiers, objectAtIndex: index];
            let kind: *mut Object = msg_send![class!(UTType), typeWithIdentifier: identifier];
            if !kind.is_null() {
                types.push(kind);
            }
        }
        types
    }
}

// Returns whether a file is one that Finder keeps out of sight.
fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.as_bytes().starts_with(b"."))
}

// Returns the name of a file as an autoreleased `NSString`.
//
// # Safety
//
// The returned string is only valid inside the current autorelease pool.
unsafe fn name_string(path: &Path) -> *mut Object {
    let name = path.file_name().unwrap_or(path.as_os_str());
    ns_string(&name.to_string_lossy())
}

// Compares two file names the way Finder sorts them, which counts the numbers in a name.
//
// # Safety
//
// Both pointers must point to valid `NSString` objects for the duration of this call.
unsafe fn compare_names(left: *mut Object, right: *mut Object) -> Ordering {
    // SAFETY: The caller supplies valid strings.
    let result: isize = unsafe { msg_send![left, localizedStandardCompare: right] };
    result.cmp(&0)
}

/// Returns the path of a file URL.
///
/// # Safety
///
/// `url` must point to a valid file `NSURL` for the duration of this call.
pub(crate) unsafe fn url_path(url: *mut Object) -> Option<PathBuf> {
    // SAFETY: The caller supplies a valid file URL, whose representation is copied before the
    // autorelease pool that owns it is drained.
    unsafe {
        let representation: *const c_char = msg_send![url, fileSystemRepresentation];
        if representation.is_null() {
            return None;
        }
        let bytes = CStr::from_ptr(representation).to_bytes();
        Some(PathBuf::from(OsStr::from_bytes(bytes)))
    }
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
    use objc2::rc::autoreleasepool;

    use super::*;

    #[test]
    fn browsing_steps_through_the_folder() {
        assert_eq!(sibling_index(3, 0, 1), Some(1));
        assert_eq!(sibling_index(3, 1, -1), Some(0));
    }

    #[test]
    fn browsing_wraps_around_at_both_ends() {
        assert_eq!(sibling_index(3, 2, 1), Some(0));
        assert_eq!(sibling_index(3, 0, -1), Some(2));
    }

    #[test]
    fn browsing_a_folder_with_one_image_stays_where_it_is() {
        assert_eq!(sibling_index(1, 0, 1), None);
        assert_eq!(sibling_index(1, 0, -1), None);
        assert_eq!(sibling_index(0, 0, 1), None);
        // A file that is no longer part of the folder has no neighbour either.
        assert_eq!(sibling_index(2, 2, 1), None);
    }

    // Creates a folder holding the given files, next to a folder of its own.
    fn folder_with(name: &str, files: &[&str]) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("imageview-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("subfolder")).expect("failed to create test folder");
        for file in files {
            std::fs::write(folder.join(file), b"").expect("failed to create test file");
        }
        folder
    }

    // Returns the types of an application that reads every image, as the bundle would declare.
    fn image_types() -> Vec<*mut Object> {
        // SAFETY: The type of images is part of the system type database.
        let kind: *mut Object =
            unsafe { msg_send![class!(UTType), typeWithIdentifier: ns_string("public.image")] };
        vec![kind]
    }

    fn names_of(siblings: &[PathBuf]) -> Vec<String> {
        siblings
            .iter()
            .map(|sibling| {
                sibling
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn a_folder_lists_its_images_the_way_finder_sorts_them() {
        autoreleasepool(|_| {
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
            // SAFETY: The types live in the autorelease pool around this call.
            let siblings = unsafe { openable_siblings(&folder.join("2.png"), &image_types()) };
            // Numbers sort naturally; hidden and unreadable files are excluded.
            assert_eq!(
                names_of(&siblings),
                ["2.png", "10.png", "photo.jfif", "photo.jpeg"]
            );
            let _ = std::fs::remove_dir_all(&folder);
        });
    }

    #[test]
    fn the_open_file_is_part_of_its_folder_whatever_it_holds() {
        autoreleasepool(|_| {
            let folder = folder_with("unknown", &["drawing.unknown", "photo.png"]);
            let open = folder.join("drawing.unknown");
            // SAFETY: The types live in the autorelease pool around this call.
            let siblings = unsafe { openable_siblings(&open, &image_types()) };
            assert_eq!(names_of(&siblings), ["drawing.unknown", "photo.png"]);
            // An unopened file of an unreadable type is excluded.
            // SAFETY: The types live in the autorelease pool around this call.
            let siblings = unsafe { openable_siblings(&folder.join("photo.png"), &image_types()) };
            assert_eq!(names_of(&siblings), ["photo.png"]);
            let _ = std::fs::remove_dir_all(&folder);
        });
    }
}
