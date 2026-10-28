/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::EXTENSIONS;

/// Returns the index `offset` places from `current`, wrapping around, or `None` when that is the
/// current item itself.
pub fn sibling_index(count: usize, current: usize, offset: isize) -> Option<usize> {
    if count == 0 || current >= count {
        return None;
    }
    let index = (current as isize + offset).rem_euclid(count as isize) as usize;
    (index != current).then_some(index)
}

/// Returns whether the file name extension is one the local decoders read.
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|supported| supported.eq_ignore_ascii_case(extension))
        })
}

/// Returns the path of the image `offset` places from `path` inside its folder. Visible files
/// that `is_openable` accepts are browsed in the order of `compare`, like [`compare_names`].
pub fn neighbour_path(
    path: &Path,
    offset: isize,
    is_openable: impl Fn(&Path) -> bool,
    compare: impl Fn(&str, &str) -> Ordering,
) -> Option<PathBuf> {
    let entries = std::fs::read_dir(path.parent()?).ok()?;
    let mut siblings: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            let sibling = entry.path();
            sibling == path
                || (!is_hidden(&sibling)
                    && entry.file_type().is_ok_and(|kind| !kind.is_dir())
                    && is_openable(&sibling))
        })
        .map(|entry| entry.path())
        .collect();
    let name = |path: &PathBuf| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    siblings.sort_by(|left, right| compare(&name(left), &name(right)));
    let current = siblings.iter().position(|sibling| sibling == path)?;
    sibling_index(siblings.len(), current, offset).map(|index| siblings[index].clone())
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

/// Compares file names like file managers do: ignoring case and with numbers in numeric order.
pub fn compare_names(left: &str, right: &str) -> Ordering {
    let (mut left, mut right) = (left.chars().peekable(), right.chars().peekable());
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let take_number = |chars: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut digits = String::new();
                    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
                        digits.push(digit);
                    }
                    digits
                };
                let (a, b) = (take_number(&mut left), take_number(&mut right));
                let (a_trimmed, b_trimmed) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
                let ordering = a_trimmed
                    .len()
                    .cmp(&b_trimmed.len())
                    .then_with(|| a_trimmed.cmp(b_trimmed));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(a), Some(b)) => {
                let ordering = a.to_lowercase().cmp(b.to_lowercase());
                if ordering != Ordering::Equal {
                    return ordering;
                }
                left.next();
                right.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(sibling_index(0, 0, 1), None);
    }

    #[test]
    fn names_sort_naturally() {
        let mut names = vec!["img10.png", "IMG2.png", "img1.png", "a.png", "img02b.png"];
        names.sort_by(|a, b| compare_names(a, b));
        assert_eq!(
            names,
            ["a.png", "img1.png", "IMG2.png", "img02b.png", "img10.png"]
        );
    }

    #[test]
    fn supported_extensions_ignore_case() {
        assert!(is_supported(Path::new("a/B.PNG")));
        assert!(is_supported(Path::new("drawing.tvg")));
        assert!(!is_supported(Path::new("notes.txt")));
        assert!(!is_supported(Path::new("png")));
    }

    #[test]
    fn neighbours_skip_other_files_and_wrap_around() {
        let folder = std::env::temp_dir().join(format!("imageview-core-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["b10.png", "b2.png", "notes.txt", ".hidden.png", "a.svg"] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        let next = |name: &str, offset| {
            neighbour_path(&folder.join(name), offset, is_supported, compare_names).and_then(
                |path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                },
            )
        };
        assert_eq!(next("a.svg", 1).as_deref(), Some("b2.png"));
        assert_eq!(next("b2.png", 1).as_deref(), Some("b10.png"));
        assert_eq!(next("b10.png", 1).as_deref(), Some("a.svg"));
        assert_eq!(next("a.svg", -1).as_deref(), Some("b10.png"));
        // The open file is part of its folder whatever it holds.
        assert_eq!(next("notes.txt", 1).as_deref(), Some("a.svg"));
        std::fs::remove_dir_all(folder).unwrap();
    }
}
