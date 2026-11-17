/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bsql::FromRow;
use from_derive::FromStruct;
use jiff::Timestamp;
use uuid::Uuid;

use crate::api;
use crate::models::user::{User, UserRole};

// MARK: Note filter fragments (used in SQL WHERE clauses)
pub(crate) const FILTER_NORMAL: &str = "is_trashed = 0 AND is_archived = 0 AND is_pinned = 0";
pub(crate) const FILTER_PINNED: &str = "is_trashed = 0 AND is_pinned = 1";
pub(crate) const FILTER_ARCHIVED: &str = "is_trashed = 0 AND is_archived = 1";
pub(crate) const FILTER_TRASHED: &str = "is_trashed = 1";

#[derive(Clone, FromRow, FromStruct)]
#[from_struct(api::Note, only_into)]
pub(crate) struct Note {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: Option<String>,
    pub body: String,
    pub is_pinned: bool,
    pub is_archived: bool,
    pub is_trashed: bool,
    pub position: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Default for Note {
    fn default() -> Self {
        let now = Timestamp::now();
        Self {
            id: Uuid::now_v7(),
            user_id: Uuid::nil(),
            title: None,
            body: String::new(),
            is_pinned: false,
            is_archived: false,
            is_trashed: false,
            position: 0,
            created_at: now,
            updated_at: now,
        }
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

    pub(crate) fn can_show(auth_user: &User, note: &Note) -> bool {
        match auth_user.role {
            UserRole::Admin => true,
            UserRole::Normal => auth_user.id == note.user_id,
        }
    }

    pub(crate) fn can_update(auth_user: &User, note: &Note) -> bool {
        match auth_user.role {
            UserRole::Admin => true,
            UserRole::Normal => auth_user.id == note.user_id,
        }
    }

    pub(crate) fn can_delete(auth_user: &User, note: &Note) -> bool {
        match auth_user.role {
            UserRole::Admin => true,
            UserRole::Normal => auth_user.id == note.user_id,
        }
    }
}
