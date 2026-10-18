/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Relative path patterns where `*` and `?` match within one path component, ignoring
//! ASCII case, like `Google/Chrome/User Data/*/Cache`.

use std::io;
use std::path::{Path, PathBuf};

/// Checks that `pattern` uses no wildcard syntax beyond `*` and `?`.
pub(crate) fn validate(pattern: &str) -> Result<(), String> {
    if let Some(special) = pattern.chars().find(|c| matches!(c, '[' | ']' | '{' | '}')) {
        return Err(format!("unsupported character `{special}`"));
    }
    if pattern.contains("**") {
        return Err("recursive `**` is not supported".to_owned());
    }
    Ok(())
}

/// Returns the existing paths under `base` that match `pattern`, sorted.
pub(crate) fn expand(base: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
    let mut paths = vec![base.to_path_buf()];
    for component in pattern.split(['/', '\\']).filter(|part| !part.is_empty()) {
        let mut next = Vec::new();
        for path in &paths {
            if !component.contains(['*', '?']) {
                let path = path.join(component);
                if path.exists() {
                    next.push(path);
                }
                continue;
            }
            let entries = match path.read_dir() {
                Ok(entries) => entries,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            for entry in entries {
                let entry = entry?;
                if matches(component, &entry.file_name().to_string_lossy()) {
                    next.push(entry.path());
                }
            }
        }
        paths = next;
    }
    paths.sort();
    Ok(paths)
}

/// Matches one path component against a pattern with `*` and `?` wildcards.
fn matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    // After a mismatch, retry from the last `*` with it covering one more character.
    let mut star = None;
    while n < name.len() {
        if pattern.get(p) == Some(&'*') {
            star = Some((p + 1, n));
            p += 1;
        } else if pattern
            .get(p)
            .is_some_and(|&c| c == '?' || c.eq_ignore_ascii_case(&name[n]))
        {
            p += 1;
            n += 1;
        } else if let Some((after_star, covered)) = star {
            star = Some((after_star, covered + 1));
            (p, n) = (after_star, covered + 1);
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn matches_wildcards_ignoring_ascii_case() {
        assert!(matches("*", ""));
        assert!(matches("*", ".hidden"));
        assert!(matches("Opera*", "opera GX Stable"));
        assert!(matches("thumbcache_*.db", "thumbcache_1024.db"));
        assert!(matches(
            "5319275A.WhatsAppDesktop_*",
            "5319275a.whatsappdesktop_cv1g1"
        ));
        assert!(matches("a?c*", "abcdef"));
        assert!(matches("*a*b", "xxaxxb"));
        assert!(!matches("thumbcache_*.db", "thumbcache_1024.dbx"));
        assert!(!matches("a?c", "ac"));
        assert!(!matches("*a*b", "xxaxx"));
    }

    #[test]
    fn rejects_unsupported_syntax() {
        assert!(validate("User Data/*/Cache").is_ok());
        assert!(validate("Cache[12]").is_err());
        assert!(validate("{a,b}").is_err());
        assert!(validate("**/Cache").is_err());
    }

    #[test]
    fn expands_existing_paths() {
        let root = std::env::temp_dir().join(format!("binman-pattern-{}", std::process::id()));
        _ = fs::remove_dir_all(&root);
        for dir in [
            "User Data/Default/Cache",
            "User Data/Profile 1/Cache",
            "User Data/Empty",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("User Data/Local State"), "").unwrap();

        let paths = expand(&root, "User Data/*/Cache").unwrap();
        assert_eq!(
            paths,
            [
                root.join("User Data/Default/Cache"),
                root.join("User Data/Profile 1/Cache")
            ]
        );
        assert!(expand(&root, "Missing/*/Cache").unwrap().is_empty());
        _ = fs::remove_dir_all(&root);
    }
}
