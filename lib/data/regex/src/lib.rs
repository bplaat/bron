/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [regex](https://crates.io/crates/regex) crate.

pub use regex_lite::*;

#[doc(hidden)]
pub mod __private {
    pub use std::sync::LazyLock;
}

/// Creates a lazily compiled regular expression from a string literal.
#[macro_export]
macro_rules! regex {
    ($pattern:literal) => {{
        static REGEX: $crate::__private::LazyLock<$crate::Regex> =
            $crate::__private::LazyLock::new(|| {
                $crate::Regex::new($pattern).expect("invalid regex pattern")
            });
        let regex: &$crate::Regex = &REGEX;
        regex
    }};
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_macro_reuses_compiled_expression() {
        fn expression() -> &'static Regex {
            regex!(r"(\d+)-(\w+)")
        }

        let first = expression();
        let second = expression();

        assert!(std::ptr::eq(first, second));
        let captures = first.captures("42-answer").expect("pattern should match");
        assert_eq!(&captures[1], "42");
        assert_eq!(&captures[2], "answer");
    }

    #[test]
    fn reexports_replacement_api() {
        let expression = Regex::new(r"(\w+)=(\d+)").expect("pattern should compile");
        assert_eq!(
            expression.replace_all("x=1 y=2", |captures: &Captures<'_>| {
                format!("{}:{}", &captures[1], &captures[2])
            }),
            "x:1 y:2"
        );
    }
}
