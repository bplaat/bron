/*
 * Copyright (c) 2024-2026 PlaatSoft
 *
 * SPDX-License-Identifier: MIT
 */

use std::time::Duration;

use anyhow::Result;
use bsql::Connection;
use chrono::Utc;
use small_http::{Request, Response, Status};

use crate::Context;
use crate::consts::{SESSION_EXPIRY_SECONDS, SESSION_REFRESH_THRESHOLD_SECONDS};
use crate::models::{Session, User};

// MARK: Auth optional
pub(crate) fn auth_optional_pre_layer(
    req: &Request,
    ctx: &mut Context,
) -> Result<Option<Response>> {
    let Some(authorization) = req.headers.get("Authorization") else {
        return Ok(None);
    };
    let Some(token) = authorization.strip_prefix("Bearer ") else {
        return Ok(None);
    };

    if let Some((session, user)) = lookup_session_and_user(token.trim(), &ctx.database)? {
        ctx.auth_session = Some(session);
        ctx.auth_user = Some(user);
    }

    Ok(None)
}

// MARK: Auth required
pub(crate) fn auth_required_pre_layer(
    req: &Request,
    ctx: &mut Context,
) -> Result<Option<Response>> {
    let authorization = match req.headers.get("Authorization") {
        Some(authorization) => authorization,
        None => {
            return Ok(Some(Response::new().status(Status::Unauthorized)));
        }
    };
    let token = match authorization.strip_prefix("Bearer ") {
        Some(t) => t.trim(),
        None => {
            return Ok(Some(Response::new().status(Status::Unauthorized)));
        }
    };

    match lookup_session_and_user(token, &ctx.database)? {
        Some((session, user)) => {
            ctx.auth_session = Some(session);
            ctx.auth_user = Some(user);
            Ok(None)
        }
        None => Ok(Some(Response::new().status(Status::Unauthorized))),
    }
}

// MARK: Utils
fn lookup_session_and_user(token: &str, db: &Connection) -> Result<Option<(Session, User)>> {
    let Some(session) = db
        .query::<Session>(
            format!(
                "SELECT {} FROM sessions WHERE token = ? AND expires_at > ? LIMIT 1",
                Session::columns()
            ),
            (token.to_string(), Utc::now()),
        )?
        .next()
        .transpose()?
    else {
        return Ok(None);
    };

    let Some(user) = db
        .query::<User>(
            format!("SELECT {} FROM users WHERE id = ? LIMIT 1", User::columns()),
            session.user_id,
        )?
        .next()
        .transpose()?
    else {
        return Ok(None);
    };

    // Sliding-window refresh: extend expiry when less than SESSION_REFRESH_THRESHOLD_SECONDS remain
    let refresh_threshold = Utc::now() + Duration::from_secs(SESSION_REFRESH_THRESHOLD_SECONDS);
    if session.expires_at.timestamp() < refresh_threshold.timestamp() {
        let new_expires_at = Utc::now() + Duration::from_secs(SESSION_EXPIRY_SECONDS);
        db.execute(
            "UPDATE sessions SET expires_at = ? WHERE token = ?",
            (new_expires_at, token.to_string()),
        )?;
    }

    Ok(Some((session, user)))
}

// MARK: Tests
#[cfg(test)]
mod test {

    use super::*;
    use crate::context::DatabaseHelpers;
    use crate::models::UserRole;
    use crate::router;
    use crate::test_utils::create_test_user_with_session_and_role;

    #[test]
    fn test_unauthed() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        let res = router.handle(Request::with_url("http://localhost/api/auth/validate"));
        assert_eq!(res.status, Status::Unauthorized);
    }

    #[test]
    fn test_authed() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        // Create a test user and session
        let (_, token) = create_test_user_with_session_and_role(&ctx, UserRole::Admin);

        // Add Authorization header to request
        let req = Request::with_url("http://localhost/api/auth/validate")
            .header("Authorization", format!("Bearer {token}"));
        let res = router.handle(req);
        assert_eq!(res.status, Status::Ok);
    }

    #[test]
    fn test_session_sliding_window_refresh() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let (user, _) = create_test_user_with_session_and_role(&ctx, UserRole::Normal);

        // Insert a session expiring in 15 days - below the 30-day refresh threshold
        let token = "sliding-window-token";
        let short_expiry = Utc::now() + Duration::from_secs(15 * 24 * 60 * 60);
        ctx.database
            .insert_session(Session {
                user_id: user.id,
                token: token.to_string(),
                expires_at: short_expiry,
                ..Default::default()
            })
            .unwrap();

        // Trigger the auth layer via any authed endpoint
        let res = router.handle(
            Request::with_url("http://localhost/api/auth/validate")
                .header("Authorization", format!("Bearer {token}")),
        );
        assert_eq!(res.status, Status::Ok);

        // The session expiry must now be approximately now + 90 days (> 80 days away)
        let min_expected = Utc::now() + Duration::from_secs(80 * 24 * 60 * 60);
        let updated_session = ctx
            .database
            .query::<Session>(
                format!(
                    "SELECT {} FROM sessions WHERE token = ? LIMIT 1",
                    Session::columns()
                ),
                token.to_string(),
            )
            .unwrap()
            .next()
            .map(|r| r.unwrap())
            .unwrap();
        assert!(
            updated_session.expires_at.timestamp() > min_expected.timestamp(),
            "expires_at should have been extended beyond 80 days"
        );
    }

    #[test]
    fn test_database_error_returns_internal_server_error() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        ctx.database
            .execute("DROP TABLE sessions", ())
            .expect("Can't remove sessions table");

        let req = Request::with_url("http://localhost/api/auth/validate")
            .header("Authorization", "Bearer test-token");
        let res = router.handle(req);

        assert_eq!(res.status, Status::InternalServerError);
    }
}
