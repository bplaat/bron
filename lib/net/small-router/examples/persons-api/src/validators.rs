/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// MARK: Name
// Blocks a specific name as an example of a custom domain validation rule.
pub(crate) fn name_validator(name: &str) -> validate::Result {
    if name.to_lowercase() == "bastiaan" {
        Err(validate::Error::new("Name can't be Bastiaan"))
    } else {
        Ok(())
    }
}

// MARK: Query
pub(crate) fn query_validator(query: &str) -> validate::Result {
    if query.chars().take(129).count() > 128 {
        Err(validate::Error::new("Must be at most 128 characters long"))
    } else {
        Ok(())
    }
}
