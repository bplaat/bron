/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use serde::Deserialize;
use validate::Validate;

use crate::api;
pub(crate) use crate::models::person::*;

mod person;

// MARK: IndexQuery
#[derive(Deserialize, Validate)]
#[serde(default)]
pub(crate) struct IndexQuery {
    #[serde(rename = "q")]
    #[validate(custom(crate::validators::query_validator))]
    pub query: Option<String>,
    #[validate(range(min = 1))]
    pub page: i64,
    #[validate(range(min = 1, max = 50))]
    pub limit: i64,
}

impl Default for IndexQuery {
    fn default() -> Self {
        Self {
            query: None,
            page: 1,
            limit: 20,
        }
    }
}

// MARK: Validate
impl From<validate::Report> for api::Report {
    fn from(report: validate::Report) -> Self {
        Self(report.0)
    }
}
