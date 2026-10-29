/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Debug-build encoding verification for `msg_send!`.

use std::ffi::CStr;

use crate::encode::Encoding;
use crate::ffi::{class_getInstanceMethod, class_getName, method_getTypeEncoding, object_getClass};
use crate::runtime::{AnyClass, AnyObject, Sel};

// Returns the byte length of one ObjC type token in `s`.
fn enc_type_len(s: &str) -> Option<usize> {
    let first = s.chars().next()?;
    match first {
        '{' => matching_close(s, '{', '}'),
        '(' => matching_close(s, '(', ')'),
        '[' => matching_close(s, '[', ']'),
        '^' | 'r' | 'n' | 'N' | 'o' | 'O' | 'R' | 'V' => enc_type_len(&s[1..]).map(|n| 1 + n),
        '@' if s.starts_with("@?") => Some(2),
        c => Some(c.len_utf8()),
    }
}

fn next_enc_type(s: &str) -> Option<(&str, &str)> {
    // Strip leading digits (frame size / stack offset from previous token)
    let s = s.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
    if s.is_empty() {
        return None;
    }
    let token_end = enc_type_len(s)?;
    let (token, rest) = s.split_at(token_end);
    // Skip trailing digits (the stack offset after this token)
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    Some((token, rest))
}

fn matching_close(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(i + c.len_utf8());
            }
        }
    }
    None
}

fn class_name(cls: *mut std::ffi::c_void) -> String {
    // SAFETY: `cls` is a non-null class pointer verified by the caller before this
    // helper is invoked; `class_getName` returns a static null-terminated C string.
    unsafe {
        CStr::from_ptr(class_getName(cls))
            .to_string_lossy()
            .into_owned()
    }
}

fn sel_name(sel: Sel) -> String {
    if sel.0.is_null() {
        return String::from("<unknown>");
    }
    // SAFETY: `sel.0` is a non-null registered selector pointer (checked above);
    // `sel_getName` returns a static null-terminated C string.
    unsafe {
        CStr::from_ptr(crate::ffi::sel_getName(sel.0))
            .to_string_lossy()
            .into_owned()
    }
}

// Strip leading ObjC type modifier chars (`r`=const, `n`/`N`/`o`/`O`/`R`/`V` = in/out/inout/bycopy/byref/oneway).
fn strip_modifiers(s: &str) -> &str {
    s.trim_start_matches(['r', 'n', 'N', 'o', 'O', 'R', 'V'])
}

// Lenient encoding comparison:
// - Leading ObjC type modifiers (`r`=const, etc.) are stripped before comparison.
// - Exact match (after stripping) always passes.
// - `^v` matches any pointer (`^...`) and vice versa.
// - Signed/unsigned variants of same-width integers are interchangeable.
fn enc_match(actual: &str, expected: &str) -> bool {
    let actual = strip_modifiers(actual);
    let expected = strip_modifiers(expected);
    if actual == expected {
        return true;
    }
    // Blocks and objects share a pointer ABI. Keep this leniency for untyped Cocoa APIs that
    // expose blocks as objects.
    if (actual == "@?" || actual == "@") && (expected == "@?" || expected == "@") {
        return true;
    }
    // Relax void-pointer: ^v matches any ^T and vice versa
    if (actual == "^v" || expected == "^v") && actual.starts_with('^') && expected.starts_with('^')
    {
        return true;
    }
    // Relax sign for integer types
    fn sign_relax(s: &str) -> &str {
        match s {
            "c" | "C" => "c",
            "s" | "S" => "s",
            "i" | "I" => "i",
            "l" | "L" => "l",
            "q" | "Q" => "q",
            other => other,
        }
    }
    if sign_relax(actual) == sign_relax(expected) {
        return true;
    }
    false
}

fn verify_method(cls: *const std::ffi::c_void, sel: Sel, args: &[Encoding], ret: &Encoding) {
    if cls.is_null() {
        return;
    }

    // SAFETY: `cls` is non-null (checked above); `sel.0` is a registered selector.
    let method = unsafe { class_getInstanceMethod(cls, sel.0) };
    if method.is_null() {
        panic!(
            "invalid message send to -[{} {}]: method not found",
            class_name(cls.cast_mut()),
            sel_name(sel),
        );
    }

    // SAFETY: `method` is non-null (checked above).
    let enc_ptr = unsafe { method_getTypeEncoding(method) };
    if enc_ptr.is_null() {
        return;
    }
    // SAFETY: `enc_ptr` is non-null and the ObjC runtime guarantees a valid null-terminated C string.
    let enc_str = unsafe { CStr::from_ptr(enc_ptr) }.to_string_lossy();
    let enc = enc_str.as_ref();

    // First token is return type
    let Some((actual_ret, mut rest)) = next_enc_type(enc) else {
        return;
    };

    let expected_ret = ret.to_string();
    if !enc_match(actual_ret, &expected_ret) {
        panic!(
            "invalid message send to -[{} {}]: expected return type '{}' but found '{}'",
            class_name(cls.cast_mut()),
            sel_name(sel),
            expected_ret,
            actual_ret,
        );
    }

    // Skip implicit receiver (`@`) and selector (`:`) args
    for _ in 0..2 {
        if let Some((_, r)) = next_enc_type(rest) {
            rest = r;
        } else {
            return;
        }
    }

    // Verify each explicit argument
    for (i, arg_enc) in args.iter().enumerate() {
        match next_enc_type(rest) {
            None => {
                panic!(
                    "invalid message send to -[{} {}]: too many arguments (method has {}, got {})",
                    class_name(cls.cast_mut()),
                    sel_name(sel),
                    i,
                    args.len(),
                );
            }
            Some((actual_arg, r)) => {
                rest = r;
                let expected_arg = arg_enc.to_string();
                if !enc_match(actual_arg, &expected_arg) {
                    panic!(
                        "invalid message send to -[{} {}]: argument {} expected '{}' but found '{}'",
                        class_name(cls.cast_mut()),
                        sel_name(sel),
                        i,
                        expected_arg,
                        actual_arg,
                    );
                }
            }
        }
    }

    // Ensure method doesn't expect more arguments
    if next_enc_type(rest).is_some() {
        panic!(
            "invalid message send to -[{} {}]: too few arguments (got {}, method expects more)",
            class_name(cls.cast_mut()),
            sel_name(sel),
            args.len(),
        );
    }
}

/// Verify that sending `sel` to `obj` with Rust types `args`/`ret` matches the ObjC method
/// signature. Panics with a descriptive message on any mismatch.
/// Called by `msg_send!` in debug builds only; zero overhead in release.
pub(crate) fn verify_send(obj: *mut AnyObject, sel: Sel, args: &[Encoding], ret: &Encoding) {
    // SAFETY: `obj` is a valid ObjC object pointer passed by `msg_send!`.
    let cls = unsafe { object_getClass(obj as *const AnyObject) };
    verify_method(cls, sel, args, ret);
}

/// Verify a super send starting lookup at `superclass`, matching `objc_msgSendSuper` semantics.
pub(crate) fn verify_super_send(
    superclass: *const AnyClass,
    sel: Sel,
    args: &[Encoding],
    ret: &Encoding,
) {
    verify_method(superclass.cast(), sel, args, ret);
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;
    use crate::runtime::AnyObject;
    use crate::{class, msg_send};

    #[link(name = "Foundation", kind = "framework")]
    unsafe extern "C" {}

    #[test]
    fn test_enc_type_len_handles_nested_types() {
        assert_eq!(enc_type_len("@?"), Some(2));
        assert_eq!(enc_type_len("^v"), Some(2));
        assert_eq!(enc_type_len("[4{CGPoint=dd}]"), Some(15));
        assert_eq!(enc_type_len("{Outer={Inner=ii}q}"), Some(19));
    }

    #[test]
    fn test_next_enc_type_skips_offsets() {
        let (ret, rest) = next_enc_type("v24@0:8q16").expect("return token");
        assert_eq!(ret, "v");

        let (receiver, rest) = next_enc_type(rest).expect("receiver token");
        assert_eq!(receiver, "@");

        let (selector, rest) = next_enc_type(rest).expect("selector token");
        assert_eq!(selector, ":");

        let (arg, rest) = next_enc_type(rest).expect("argument token");
        assert_eq!(arg, "q");
        assert_eq!(rest, "");
    }

    #[test]
    fn test_matching_close_and_strip_modifiers() {
        assert_eq!(matching_close("{foo={bar=ii}q}", '{', '}'), Some(15));
        assert_eq!(matching_close("[2[3i]]", '[', ']'), Some(7));
        assert_eq!(strip_modifiers("rn^v"), "^v");
    }

    #[test]
    fn test_enc_match_relaxed_rules() {
        assert!(enc_match("r^i", "^v"));
        assert!(enc_match("@?", "@"));
        assert!(enc_match("Q", "q"));
        assert!(!enc_match("i", "d"));
        // BOOL must match the current target. Rust bool cannot accept every i8 bit pattern.
        assert!(!enc_match("B", "c"));
        assert!(!enc_match("c", "B"));
        assert!(enc_match("B", "B"));
        assert!(enc_match("{CGPoint=dd}", "{CGPoint=dd}"));
        assert!(!enc_match("{CGPoint=dd}", "{CGPoint=ff}"));
        assert!(!enc_match("{CGPoint=dd}", "{CGSize=dd}"));
    }

    #[test]
    fn test_sel_name_null_selector() {
        assert_eq!(sel_name(Sel(std::ptr::null())), "<unknown>");
    }

    #[test]
    fn test_class_name_known_class() {
        assert_eq!(class_name(class!(NSObject) as *mut _), "NSObject");
    }

    #[test]
    fn test_verify_super_send_uses_superclass() {
        verify_super_send(
            class!(NSObject).cast::<AnyClass>(),
            crate::sel!(description),
            &[],
            &Encoding::Object,
        );
    }

    #[test]
    #[should_panic(expected = "invalid message send")]
    fn test_verify_wrong_return_type() {
        // NSString's -length returns NSUInteger (Q), not a pointer object (@)
        // SAFETY: NSString is a valid Foundation class; alloc returns a valid uninitialized object.
        let ns: *mut AnyObject = unsafe { msg_send![class!(NSString), alloc] };
        // SAFETY: `ns` is a valid uninitialized NSString from alloc; init is always valid.
        let ns: *mut AnyObject = unsafe { msg_send![ns, init] };
        // SAFETY: `ns` is a valid ObjC object; intentionally wrong return type triggers the panic.
        let _wrong: *mut AnyObject = unsafe { msg_send![ns, length] };
    }
}
