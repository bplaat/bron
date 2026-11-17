/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bsql::FromRow;
use jiff::Timestamp;
use uuid::Uuid;

use crate::api;
use crate::models::{User, UserRole};

#[derive(Clone, FromRow)]
pub(crate) struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token: String,
    pub ip_address: String,
    pub ip_latitude: Option<f64>,
    pub ip_longitude: Option<f64>,
    pub ip_country: Option<String>,
    pub ip_city: Option<String>,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub client_os: Option<String>,
    pub expires_at: Timestamp,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Default for Session {
    fn default() -> Self {
        let now = Timestamp::now();
        Self {
            id: Uuid::now_v7(),
            user_id: Uuid::nil(),
            token: String::new(),
            ip_address: String::new(),
            ip_latitude: None,
            ip_longitude: None,
            ip_country: None,
            ip_city: None,
            client_name: None,
            client_version: None,
            client_os: None,
            expires_at: now,
            created_at: now,
            updated_at: now,
        }
    }
}

impl From<Session> for api::Session {
    fn from(session: Session) -> Self {
        Self {
            id: session.id,
            user_id: session.user_id,
            ip: api::SessionIp {
                address: Some(session.ip_address),
                latitude: session.ip_latitude,
                longitude: session.ip_longitude,
                country: session.ip_country,
                city: session.ip_city,
            },
            client: api::SessionClient {
                name: session.client_name,
                version: session.client_version,
                os: session.client_os,
            },
            expires_at: session.expires_at,
            created_at: session.created_at,
            updated_at: session.updated_at,
        }
    }
}

// MARK: Policies
pub(crate) mod policies {
    use super::*;

    pub(crate) fn can_index(auth_user: &User) -> bool {
        auth_user.role == UserRole::Admin
    }

    pub(crate) fn can_show(auth_user: &User, session: &Session) -> bool {
        match auth_user.role {
            UserRole::Admin => true,
            UserRole::Normal => auth_user.id == session.user_id,
        }
    }

    pub(crate) fn can_delete(auth_user: &User, session: &Session) -> bool {
        match auth_user.role {
            UserRole::Admin => true,
            UserRole::Normal => auth_user.id == session.user_id,
        }
    }
}
