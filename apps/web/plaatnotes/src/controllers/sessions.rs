/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use anyhow::Result;
use bsql::{execute_args, preprocess_fts_query, query_args};
use chrono::Utc;
use small_http::{Request, Response, Status};
use uuid::Uuid;

use crate::api;
use crate::context::Context;
use crate::controllers::users::{get_user, parse_user_id};
use crate::controllers::{not_found, parse_index_query, require_auth};
use crate::models::session::policies;
use crate::models::{IndexQuery, Session, UserRole};

// MARK: Handlers
pub(crate) fn sessions_index(req: &Request, ctx: &Context) -> Result<Response> {
    let auth_user = require_auth!(ctx);

    if !policies::can_index(auth_user) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    let query = parse_index_query!(req);
    let (total, sessions) = list_all_sessions(ctx, &query)?;

    Ok(Response::with_json(api::SessionIndexResponse {
        pagination: api::Pagination {
            page: query.page,
            limit: query.limit,
            total,
        },
        data: sessions,
    }))
}

pub(crate) fn sessions_show(req: &Request, ctx: &Context) -> Result<Response> {
    let auth_user = require_auth!(ctx);

    // Get session
    let session_id = match parse_session_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let session = match get_session(session_id, ctx)? {
        Some(session) => session,
        None => return not_found(req, ctx),
    };

    // Check authorization
    if !policies::can_show(auth_user, &session) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    // Return session
    Ok(Response::with_json(api::Session::from(session)))
}

pub(crate) fn sessions_delete(req: &Request, ctx: &Context) -> Result<Response> {
    let auth_user = require_auth!(ctx);

    // Get session
    let session_id = match parse_session_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let session = match get_session(session_id, ctx)? {
        Some(session) => session,
        None => return not_found(req, ctx),
    };

    // Check authorization
    if !policies::can_delete(auth_user, &session) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    if auth_user.role == UserRole::Admin {
        ctx.database
            .execute("DELETE FROM sessions WHERE id = ?", session.id)?;
    } else {
        let now = Utc::now();
        execute_args!(
            ctx.database,
            "UPDATE sessions SET expires_at = :now, updated_at = :now WHERE id = :id",
            Args {
                now: now,
                id: session.id
            }
        )?;
    }

    // Success response
    Ok(Response::with_status(Status::NoContent))
}

pub(crate) fn users_sessions(req: &Request, ctx: &Context) -> Result<Response> {
    sessions_for_user(req, ctx)
}

// MARK: Utils
fn list_sessions(
    ctx: &Context,
    user_id: Uuid,
    query: &IndexQuery,
) -> Result<(i64, Vec<api::Session>)> {
    let now = Utc::now();
    let offset = (query.page - 1) * query.limit;
    if let Some(q) = query.query.as_deref().filter(|s| !s.is_empty()) {
        let fts_query = preprocess_fts_query(q);
        let total = ctx.database.query_some::<i64>(
            "SELECT COUNT(id) FROM sessions WHERE user_id = ? AND expires_at > ? AND id IN (SELECT id FROM sessions_fts WHERE sessions_fts MATCH ?)",
            (user_id, now, fts_query.clone()),
        )?;
        let sessions = query_args!(
            Session, ctx.database,
            format!("SELECT {} FROM sessions WHERE user_id = :user_id AND expires_at > :now AND id IN (SELECT id FROM sessions_fts WHERE sessions_fts MATCH :fts_query) ORDER BY created_at DESC LIMIT :limit OFFSET :offset", Session::columns()),
            Args { user_id: user_id, now: now, fts_query: fts_query, limit: query.limit, offset: offset }
        )?.map(|r| r.map(Into::into)).collect::<Result<Vec<_>, _>>()?;
        Ok((total, sessions))
    } else {
        let total = ctx.database.query_some::<i64>(
            "SELECT COUNT(id) FROM sessions WHERE user_id = ? AND expires_at > ?",
            (user_id, now),
        )?;
        let sessions = query_args!(
            Session, ctx.database,
            format!("SELECT {} FROM sessions WHERE user_id = :user_id AND expires_at > :now ORDER BY created_at DESC LIMIT :limit OFFSET :offset", Session::columns()),
            Args { user_id: user_id, now: now, limit: query.limit, offset: offset }
        )?.map(|r| r.map(Into::into)).collect::<Result<Vec<_>, _>>()?;
        Ok((total, sessions))
    }
}

fn list_all_sessions(ctx: &Context, query: &IndexQuery) -> Result<(i64, Vec<api::Session>)> {
    let offset = (query.page - 1) * query.limit;
    if let Some(q) = query.query.as_deref().filter(|s| !s.is_empty()) {
        let fts_query = preprocess_fts_query(q);
        let total = ctx.database.query_some::<i64>(
            "SELECT COUNT(id) FROM sessions WHERE id IN (SELECT id FROM sessions_fts WHERE sessions_fts MATCH ?)",
            fts_query.clone(),
        )?;
        let sessions = query_args!(
            Session, ctx.database,
            format!("SELECT {} FROM sessions WHERE id IN (SELECT id FROM sessions_fts WHERE sessions_fts MATCH :fts_query) ORDER BY created_at DESC LIMIT :limit OFFSET :offset", Session::columns()),
            Args { fts_query: fts_query, limit: query.limit, offset: offset }
        )?.map(|r| r.map(Into::into)).collect::<Result<Vec<_>, _>>()?;
        Ok((total, sessions))
    } else {
        let total = ctx
            .database
            .query_some::<i64>("SELECT COUNT(id) FROM sessions", ())?;
        let sessions = query_args!(
            Session,
            ctx.database,
            format!(
                "SELECT {} FROM sessions ORDER BY created_at DESC LIMIT :limit OFFSET :offset",
                Session::columns()
            ),
            Args {
                limit: query.limit,
                offset: offset
            }
        )?
        .map(|r| r.map(Into::into))
        .collect::<Result<Vec<_>, _>>()?;
        Ok((total, sessions))
    }
}

fn sessions_for_user(req: &Request, ctx: &Context) -> Result<Response> {
    let auth_user = require_auth!(ctx);
    let query = parse_index_query!(req);

    // Get target user
    let user_id = match parse_user_id(req) {
        Ok(id) => id,
        Err(_) => return Ok(Response::with_status(Status::BadRequest)),
    };
    let user = match get_user(user_id, ctx)? {
        Some(user) => user,
        None => return not_found(req, ctx),
    };

    // Check authorization
    if !crate::models::user::policies::can_show(auth_user, &user) {
        return Ok(Response::with_status(Status::Forbidden));
    }

    let (total, sessions) = list_sessions(ctx, user.id, &query)?;

    Ok(Response::with_json(api::SessionIndexResponse {
        pagination: api::Pagination {
            page: query.page,
            limit: query.limit,
            total,
        },
        data: sessions,
    }))
}

fn parse_session_id(req: &Request) -> Result<Uuid> {
    match req
        .params
        .get("session_id")
        .and_then(|id| id.parse::<Uuid>().ok())
    {
        Some(id) => Ok(id),
        None => anyhow::bail!("Invalid UUID"),
    }
}

fn get_session(session_id: Uuid, ctx: &Context) -> Result<Option<Session>> {
    Ok(ctx
        .database
        .query::<Session>(
            format!(
                "SELECT {} FROM sessions WHERE id = ? LIMIT 1",
                Session::columns()
            ),
            session_id,
        )?
        .next()
        .transpose()?)
}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::time::Duration;

    use super::*;
    use crate::context::DatabaseHelpers;
    use crate::models::UserRole;
    use crate::router;
    use crate::test_utils::{
        create_test_user_with_session_and_role, insert_test_session, insert_test_user,
    };

    #[test]
    fn test_sessions_index_search() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_user1, token1) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let user2 = insert_test_user(&ctx, "Jane", "Doe", "jane@example.com");

        // Session with recognizable client_name and country
        ctx.database
            .insert_session(Session {
                user_id: user2.id,
                token: "token-jane".to_string(),
                ip_address: "1.2.3.4".to_string(),
                ip_country: Some("Netherlands".to_string()),
                client_name: Some("Firefox".to_string()),
                expires_at: Utc::now() + Duration::from_secs(crate::consts::SESSION_EXPIRY_SECONDS),
                ..Default::default()
            })
            .unwrap();
        // Session with different client
        ctx.database
            .insert_session(Session {
                user_id: user2.id,
                token: "token-jane2".to_string(),
                ip_address: "5.6.7.8".to_string(),
                client_name: Some("Chrome".to_string()),
                expires_at: Utc::now() + Duration::from_secs(crate::consts::SESSION_EXPIRY_SECONDS),
                ..Default::default()
            })
            .unwrap();

        // Search by client_name
        let res = router.handle(
            Request::get("http://localhost/api/sessions?q=Firefox")
                .header("Authorization", format!("Bearer {token1}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::SessionIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].client.name, Some("Firefox".to_string()));

        // Search by ip_country
        let res = router.handle(
            Request::get("http://localhost/api/sessions?q=Netherlands")
                .header("Authorization", format!("Bearer {token1}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::SessionIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert_eq!(response.data[0].ip.country, Some("Netherlands".to_string()));
    }

    #[test]
    fn test_sessions_index_admin() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_user1, token1) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let user2 = insert_test_user(&ctx, "Jane", "Doe", "jane@example.com");
        insert_test_session(&ctx, user2.id, "token-jane");

        let res = router.handle(
            Request::get("http://localhost/api/sessions")
                .header("Authorization", format!("Bearer {token1}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::SessionIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 2);
    }

    #[test]
    fn test_sessions_index_forbidden_for_normal_user() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token1) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);

        // Normal user gets Forbidden on /api/sessions
        let res = router.handle(
            Request::get("http://localhost/api/sessions")
                .header("Authorization", format!("Bearer {token1}")),
        );
        assert_eq!(res.status, Status::Forbidden);
    }

    #[test]
    fn test_sessions_show_admin() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token_admin) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let user = insert_test_user(&ctx, "Jane", "Doe", "jane@example.com");
        let session = insert_test_session(&ctx, user.id, "token-jane");

        let res = router.handle(
            Request::get(format!("http://localhost/api/sessions/{}", session.id))
                .header("Authorization", format!("Bearer {token_admin}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::Session>(&res.body).unwrap();
        assert_eq!(response.id, session.id);
    }

    #[test]
    fn test_sessions_show_own_session() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);
        let session = insert_test_session(&ctx, user.id, "token-other");

        let res = router.handle(
            Request::get(format!("http://localhost/api/sessions/{}", session.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::Session>(&res.body).unwrap();
        assert_eq!(response.id, session.id);
    }

    #[test]
    fn test_sessions_show_forbidden() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token_user1) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);
        let user2 = insert_test_user(&ctx, "Jane", "Doe", "jane@example.com");
        let session2 = insert_test_session(&ctx, user2.id, "token-jane");

        let res = router.handle(
            Request::get(format!("http://localhost/api/sessions/{}", session2.id))
                .header("Authorization", format!("Bearer {token_user1}")),
        );
        assert_eq!(res.status, Status::Forbidden);
    }

    #[test]
    fn test_sessions_delete_revokes_session() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);
        let session = insert_test_session(&ctx, user.id, "token-to-revoke");

        let res = router.handle(
            Request::delete(format!("http://localhost/api/sessions/{}", session.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::NoContent);

        // Verify session is retained but expired
        let revoked = query_args!(
            Session,
            ctx.database,
            format!(
                "SELECT {} FROM sessions WHERE id = :id LIMIT 1",
                Session::columns()
            ),
            Args { id: session.id }
        )
        .unwrap()
        .next()
        .map(|r| r.unwrap());
        let revoked = revoked.expect("Session history should be retained");
        assert!(revoked.expires_at.timestamp() <= Utc::now().timestamp());
        assert!(revoked.updated_at.timestamp() >= session.updated_at.timestamp());
    }

    #[test]
    fn test_sessions_delete_admin_deletes_session() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let user = insert_test_user(&ctx, "Jane", "Doe", "jane@example.com");
        let session = insert_test_session(&ctx, user.id, "token-to-delete");

        let res = router.handle(
            Request::delete(format!("http://localhost/api/sessions/{}", session.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::NoContent);
        assert!(get_session(session.id, &ctx).unwrap().is_none());
    }

    #[test]
    fn test_sessions_delete_forbidden() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token_user1) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);
        let user2 = insert_test_user(&ctx, "Jane", "Doe", "jane@example.com");
        let session2 = insert_test_session(&ctx, user2.id, "token-jane");

        let res = router.handle(
            Request::delete(format!("http://localhost/api/sessions/{}", session2.id))
                .header("Authorization", format!("Bearer {token_user1}")),
        );
        assert_eq!(res.status, Status::Forbidden);

        // Verify session still exists
        let existing = query_args!(
            Session,
            ctx.database,
            format!(
                "SELECT {} FROM sessions WHERE id = :id LIMIT 1",
                Session::columns()
            ),
            Args { id: session2.id }
        )
        .unwrap()
        .next()
        .map(|r| r.unwrap());
        assert!(existing.is_some());
    }

    #[test]
    fn test_users_sessions_admin() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token_admin) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);
        let user = insert_test_user(&ctx, "Jane", "Doe", "jane@example.com");
        for i in 0..2 {
            insert_test_session(&ctx, user.id, &format!("token-jane-{i}"));
        }

        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/sessions", user.id))
                .header("Authorization", format!("Bearer {token_admin}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::SessionIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 2);
        assert!(response.data.iter().all(|s| s.user_id == user.id));
    }

    #[test]
    fn test_users_sessions_own() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);
        insert_test_session(&ctx, user.id, "token-second");

        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/sessions", user.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::SessionIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 2);
    }

    #[test]
    fn test_users_sessions_forbidden() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token_user1) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);
        let (user2, _) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);

        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/sessions", user2.id))
                .header("Authorization", format!("Bearer {token_user1}")),
        );
        assert_eq!(res.status, Status::Forbidden);
    }

    #[test]
    fn test_users_sessions_not_found() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (_, token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);

        let res = router.handle(
            Request::get(format!(
                "http://localhost/api/users/{}/sessions",
                Uuid::now_v7()
            ))
            .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::NotFound);
    }

    #[test]
    fn test_users_sessions_filters_expired() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, token) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);

        // Expired session - can't use insert_test_session since it sets a future expiry
        ctx.database
            .insert_session(Session {
                user_id: user.id,
                token: "token-expired".to_string(),
                expires_at: Utc::now() - Duration::from_secs(3600),
                ..Default::default()
            })
            .unwrap();

        let res = router.handle(
            Request::get(format!("http://localhost/api/users/{}/sessions", user.id))
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::SessionIndexResponse>(&res.body).unwrap();
        assert_eq!(response.data.len(), 1);
        assert!(response.data.iter().all(|s| s.user_id == user.id));
    }
}
