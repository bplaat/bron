/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bsql::{FromRow, FromValue};
use from_derive::FromEnum;
use jiff::Timestamp;
use uuid::Uuid;

use crate::api;
use crate::context::Context;

// MARK: User
#[derive(Clone, FromRow)]
pub(crate) struct User {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub password: String,
    pub theme: UserTheme,
    pub language: String,
    pub role: UserRole,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Default for User {
    fn default() -> Self {
        let now = Timestamp::now();
        Self {
            id: Uuid::now_v7(),
            first_name: String::new(),
            last_name: String::new(),
            email: String::new(),
            password: String::new(),
            theme: UserTheme::System,
            language: "en".to_string(),
            role: UserRole::Normal,
            created_at: now,
            updated_at: now,
        }
    }
}

impl From<User> for api::User {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            first_name: user.first_name,
            last_name: user.last_name,
            email: user.email,
            theme: user.theme.into(),
            language: user.language,
            role: user.role.into(),
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }
}

// MARK: UserTheme
#[derive(Copy, Clone, PartialEq, Eq, FromValue, FromEnum)]
#[from_enum(api::UserTheme)]
pub(crate) enum UserTheme {
    System = 0,
    Light = 1,
    Dark = 2,
}

// MARK: UserRole
#[derive(Copy, Clone, PartialEq, Eq, FromValue, FromEnum)]
#[from_enum(api::UserRole)]
pub(crate) enum UserRole {
    Normal = 0,
    Admin = 1,
}

// MARK: Validators
pub(crate) mod validators {
    use super::*;

    pub(crate) fn is_unique_email(value: &str, context: &Context) -> validate::Result {
        let count = context
            .database
            .query_some::<i64>(
                "SELECT COUNT(id) FROM users WHERE email = ?",
                value.to_string(),
            )
            .expect("Database error");
        if count != 0 {
            return Err(validate::Error::new("Email not unique"));
        }
        Ok(())
    }

    pub(crate) fn is_unique_email_or_target_user_email(
        value: &str,
        context: &Context,
    ) -> validate::Result {
        // Allow keeping the same email as the user being updated
        if let Some(target_id) = context.update_target_user_id {
            let count = context
                .database
                .query_some::<i64>(
                    "SELECT COUNT(id) FROM users WHERE email = ? AND id != ?",
                    (value.to_string(), target_id),
                )
                .expect("Database error");
            if count != 0 {
                return Err(validate::Error::new("Email not unique"));
            }
            return Ok(());
        }
        is_unique_email(value, context)
    }

    /// Validate password complexity: requires at least one uppercase letter, one lowercase letter,
    /// one digit, and one special character (anything that is not alphanumeric).
    pub(crate) fn has_password_complexity(value: &str) -> validate::Result {
        let has_upper = value.chars().any(|c| c.is_ascii_uppercase());
        let has_lower = value.chars().any(|c| c.is_ascii_lowercase());
        let has_digit = value.chars().any(|c| c.is_ascii_digit());
        let has_special = value.chars().any(|c| !c.is_alphanumeric());
        if has_upper && has_lower && has_digit && has_special {
            Ok(())
        } else {
            Err(validate::Error::new(
                "Must contain at least one uppercase letter, one lowercase letter, one digit, and one special character",
            ))
        }
    }

    // Context-aware wrapper so complexity can be used on context-validated structs
    pub(crate) fn has_password_complexity_ctx(value: &str, _ctx: &Context) -> validate::Result {
        has_password_complexity(value)
    }
}

// MARK: Policies
pub(crate) mod policies {
    use super::*;

    pub(crate) fn can_index(auth_user: &User) -> bool {
        auth_user.role == UserRole::Admin
    }

    pub(crate) fn can_create(auth_user: &User) -> bool {
        auth_user.role == UserRole::Admin
    }

    pub(crate) fn can_show(auth_user: &User, user: &User) -> bool {
        match auth_user.role {
            UserRole::Admin => true,
            UserRole::Normal => auth_user.id == user.id,
        }
    }

    pub(crate) fn can_update(auth_user: &User, user: &User) -> bool {
        match auth_user.role {
            UserRole::Admin => true,
            UserRole::Normal => auth_user.id == user.id,
        }
    }

    pub(crate) fn can_delete(auth_user: &User, user: &User) -> bool {
        match auth_user.role {
            UserRole::Admin => true,
            UserRole::Normal => auth_user.id == user.id,
        }
    }

    pub(crate) fn can_login_as(auth_user: &User) -> bool {
        auth_user.role == UserRole::Admin
    }
}
