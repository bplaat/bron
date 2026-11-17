/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bsql::{FromRow, FromValue};
use from_derive::{FromEnum, FromStruct};
use jiff::Timestamp;
use uuid::Uuid;

use crate::api;

// MARK: Person
#[derive(Clone, FromRow, FromStruct)]
#[from_struct(api::Person, only_into)]
pub(crate) struct Person {
    pub id: Uuid,
    pub name: String,
    #[sql(rename = "age")]
    pub age_in_years: i64,
    pub relation: Relation,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Default for Person {
    fn default() -> Self {
        let now = Timestamp::now();
        Self {
            id: Uuid::now_v7(),
            name: String::new(),
            age_in_years: 0,
            relation: Relation::Me,
            created_at: now,
            updated_at: now,
        }
    }
}

// MARK: Relation
#[derive(Copy, Clone, FromEnum, FromValue)]
#[from_enum(api::Relation)]
pub(crate) enum Relation {
    Me = 0,
    Brother = 1,
    Sister = 2,
}
