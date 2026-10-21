/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Result;
use base64::prelude::*;
use bsql::execute_args;
use chrono::Utc;
use from_derive::FromStruct;
use serde::Deserialize;
use simple_useragent::UserAgentParser;
use small_http::{Request, Response, Status};
use uuid::Uuid;
use validate::Validate;
use zeroize::Zeroizing;

use crate::api;
use crate::consts::{SESSION_EXPIRY_SECONDS, SESSION_TOKEN_LENGTH};
use crate::context::{Context, DatabaseHelpers};
use crate::controllers::parse_body;
use crate::models::{Session, User};

// MARK: Handlers
static USER_AGENT_PARSER: LazyLock<UserAgentParser> = LazyLock::new(UserAgentParser::new);
static DUMMY_PASSWORD_HASH: LazyLock<String> =
    LazyLock::new(|| crate::utils::password_hash("invalid account password"));

#[derive(Deserialize)]
struct IpInfo {
    city: String,
    country: String,
    loc: String,
}

#[derive(Validate, FromStruct)]
#[from_struct(api::LoginBody, only_from)]
struct LoginBody {
    #[validate(email)]
    email: String,
    #[validate(ascii, length(min = 8, max = 128))]
    password: Zeroizing<String>,
}

pub(crate) fn auth_login(req: &Request, ctx: &Context) -> Result<Response> {
    // Parse and validate body
    let body = parse_body!(req, api::LoginBody, LoginBody);

    // Check login rate limit
    let ip_address = req
        .ip_from_trusted_proxies(&ctx.trusted_proxies)
        .to_string();
    if !ctx.is_e2e {
        let mut attempts = ctx.login_attempts.lock().unwrap_or_else(|p| p.into_inner());
        let now = std::time::Instant::now();
        let window = Duration::from_secs(crate::consts::LOGIN_RATE_LIMIT_WINDOW_SECONDS);
        attempts.retain(|_, (_, window_start)| now.duration_since(*window_start) < window);

        if let Some((count, window_start)) = attempts.get_mut(&ip_address) {
            if now.duration_since(*window_start) < window {
                *count += 1;
                if *count > crate::consts::LOGIN_RATE_LIMIT_MAX_ATTEMPTS {
                    return Ok(Response::with_status(Status::TooManyRequests));
                }
            } else {
                *count = 1;
                *window_start = now;
            }
        } else {
            attempts.insert(ip_address.clone(), (1, now));
        }
    }

    // Find user by email
    let user = match ctx
        .database
        .query::<User>(
            format!(
                "SELECT {} FROM users WHERE email = ? LIMIT 1",
                User::columns()
            ),
            body.email,
        )?
        .next()
        .transpose()?
    {
        Some(user) => user,
        None => {
            _ = verify_password(&body.password, &DUMMY_PASSWORD_HASH)?;
            return Ok(Response::with_status(Status::Unauthorized));
        }
    };

    // Verify password
    if let Some(err) = verify_password(&body.password, &user.password)? {
        return Ok(err);
    }

    // Create a new session for the user
    let token = create_session(req, ctx, user.id)?;

    // Clear rate limit counter on successful login
    if !ctx.is_e2e {
        ctx.login_attempts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&ip_address);
    }

    // Return session token
    Ok(Response::with_json(api::LoginResponse {
        user_id: user.id,
        token,
    }))
}

pub(crate) fn auth_validate(_req: &Request, ctx: &Context) -> Result<Response> {
    let user = ctx
        .auth_user
        .clone()
        .ok_or_else(|| anyhow::anyhow!("auth context missing user"))?;
    let session = ctx
        .auth_session
        .clone()
        .ok_or_else(|| anyhow::anyhow!("auth context missing session"))?;
    Ok(Response::with_json(api::AuthValidateResponse {
        user: user.into(),
        session: session.into(),
    }))
}

pub(crate) fn auth_logout(_req: &Request, ctx: &Context) -> Result<Response> {
    let token = ctx
        .auth_session
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("auth context missing session"))?
        .token
        .clone();

    // Expire the session by setting expires_at to now
    execute_args!(
        ctx.database,
        "UPDATE sessions SET expires_at = :now, updated_at = :now WHERE token = :token",
        Args {
            now: Utc::now(),
            token: token
        }
    )?;

    Ok(Response::with_status(Status::NoContent))
}

// MARK: Utils
/// Create a new session for the given user and return its token. The session is enriched with
/// IP geolocation and User-Agent information derived from the request.
pub(crate) fn create_session(req: &Request, ctx: &Context, user_id: Uuid) -> Result<String> {
    // Generate secure random token
    let token = {
        let mut bytes = [0u8; SESSION_TOKEN_LENGTH];
        getrandom::fill(&mut bytes)?;
        BASE64_URL_SAFE_NO_PAD.encode(bytes)
    };

    // Get IP information
    let ip_address = req
        .ip_from_trusted_proxies(&ctx.trusted_proxies)
        .to_string();
    let (ip_latitude, ip_longitude, ip_country, ip_city) = {
        if let Some(reader) = ctx.maxminddb_reader.get() {
            // Use local MaxMind DB
            match ip_address.parse::<std::net::IpAddr>() {
                Ok(ip) => match reader.lookup(ip) {
                    Ok(result) => match result.decode::<maxminddb::geoip2::City>() {
                        Ok(Some(city)) => (
                            city.location.latitude,
                            city.location.longitude,
                            city.country.iso_code,
                            city.city.names.english,
                        ),
                        _ => (None, None, None, None),
                    },
                    Err(_) => (None, None, None, None),
                },
                Err(_) => (None, None, None, None),
            }
        } else {
            // Fall back to ipinfo.io
            match Request::get(format!("https://ipinfo.io/{ip_address}/json")).fetch() {
                Ok(res) => {
                    if let Ok(ip_info) = serde_json::from_slice::<IpInfo>(&res.body) {
                        let (lat, lon) =
                            if let Some((lat_str, lon_str)) = ip_info.loc.split_once(',') {
                                (lat_str.parse::<f64>().ok(), lon_str.parse::<f64>().ok())
                            } else {
                                (None, None)
                            };
                        (lat, lon, Some(ip_info.country), Some(ip_info.city))
                    } else {
                        (None, None, None, None)
                    }
                }
                Err(_) => (None, None, None, None),
            }
        }
    };

    // Parse User-Agent header
    let (client_name, client_version, client_os) =
        if let Some(ua_str) = req.headers.get("User-Agent") {
            let ua = USER_AGENT_PARSER.parse(ua_str);
            (
                Some(ua.client.family),
                ua.client.version,
                Some(ua.os.family),
            )
        } else {
            (None, None, None)
        };

    // Create session
    let session = Session {
        user_id,
        token: token.clone(),
        ip_address,
        ip_latitude,
        ip_longitude,
        ip_country,
        ip_city,
        client_name,
        client_version,
        client_os,
        expires_at: Utc::now() + Duration::from_secs(SESSION_EXPIRY_SECONDS),
        ..Default::default()
    };
    ctx.database.insert_session(session)?;

    Ok(token)
}

pub(crate) fn verify_password(plain: &str, hash: &str) -> Result<Option<Response>> {
    match pbkdf2::password_verify(plain, hash) {
        Ok(true) => Ok(None),
        Ok(false) => Ok(Some(Response::with_status(Status::Unauthorized))),
        Err(_) => Ok(Some(Response::with_status(Status::InternalServerError))),
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use bsql::query_args;

    use super::*;
    use crate::consts::LOGIN_RATE_LIMIT_MAX_ATTEMPTS;
    use crate::router;
    use crate::test_utils::{insert_test_session, insert_test_user};

    #[test]
    fn test_auth_login() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        insert_test_user(&ctx, "John", "Doe", "john@example.com");

        let res = router.handle(
            Request::post("http://localhost/api/auth/login")
                .header("Content-Type", "application/json")
                .body(r#"{"email":"john@example.com","password":"password123"}"#),
        );
        assert_eq!(res.status, Status::Ok);
        let response = serde_json::from_slice::<api::LoginResponse>(&res.body).unwrap();
        assert!(!response.token.is_empty());
    }

    #[test]
    fn test_auth_login_incorrect_password() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        insert_test_user(&ctx, "John", "Doe", "john@example.com");

        let res = router.handle(
            Request::post("http://localhost/api/auth/login")
                .header("Content-Type", "application/json")
                .body(r#"{"email":"john@example.com","password":"wrongpassword"}"#),
        );
        assert_eq!(res.status, Status::Unauthorized);
    }

    #[test]
    fn test_auth_login_non_existent_email() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        let res = router.handle(
            Request::post("http://localhost/api/auth/login")
                .header("Content-Type", "application/json")
                .body(r#"{"email":"notfound@example.com","password":"password123"}"#),
        );
        assert_eq!(res.status, Status::Unauthorized);
    }

    #[test]
    fn test_auth_logout() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let user = insert_test_user(&ctx, "John", "Doe", "john@example.com");
        insert_test_session(&ctx, user.id, "test-token-123");

        let res = router.handle(
            Request::post("http://localhost/api/auth/logout")
                .header("Authorization", "Bearer test-token-123"),
        );
        assert_eq!(res.status, Status::NoContent);
        let expired_session = query_args!(
            Session,
            ctx.database,
            format!(
                "SELECT {} FROM sessions WHERE token = :token LIMIT 1",
                Session::columns()
            ),
            Args {
                token: "test-token-123".to_string()
            }
        )
        .unwrap()
        .next()
        .map(|r| r.unwrap())
        .unwrap();
        assert!(expired_session.expires_at.timestamp() <= Utc::now().timestamp());
    }

    #[test]
    fn test_get_authenticated_user_valid_token() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let user = insert_test_user(&ctx, "John", "Doe", "john@example.com");
        insert_test_session(&ctx, user.id, "valid-token-456");

        let res = router.handle(
            Request::get("http://localhost/api/auth/validate")
                .header("Authorization", "Bearer valid-token-456"),
        );
        assert_eq!(res.status, Status::Ok);
    }

    #[test]
    fn test_get_authenticated_user_invalid_token() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        let res = router.handle(
            Request::get("http://localhost/api/auth/validate")
                .header("Authorization", "Bearer invalid-token"),
        );
        assert_eq!(res.status, Status::Unauthorized);
    }

    #[test]
    fn test_get_authenticated_user_expired_session() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        let user = insert_test_user(&ctx, "John", "Doe", "john@example.com");

        // Expired session - can't use insert_test_session since it sets a future expiry
        ctx.database
            .insert_session(Session {
                user_id: user.id,
                token: "expired-token-789".to_string(),
                expires_at: Utc::now() - Duration::from_secs(3600),
                ..Default::default()
            })
            .unwrap();

        let res = router.handle(
            Request::get("http://localhost/api/auth/validate")
                .header("Authorization", "Bearer expired-token-789"),
        );
        assert_eq!(res.status, Status::Unauthorized);
    }

    #[test]
    fn test_auth_login_rate_limiting() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());
        insert_test_user(&ctx, "Jane", "Doe", "jane@example.com");

        // The first LOGIN_RATE_LIMIT_MAX_ATTEMPTS attempts should proceed (wrong password returns 401)
        for _ in 0..LOGIN_RATE_LIMIT_MAX_ATTEMPTS {
            let res = router.handle(
                Request::post("http://localhost/api/auth/login")
                    .header("Content-Type", "application/json")
                    .body(r#"{"email":"jane@example.com","password":"wrongpassword"}"#),
            );
            assert_eq!(res.status, Status::Unauthorized);
        }

        // The next attempt must be rejected with 429 before even checking credentials
        let res = router.handle(
            Request::post("http://localhost/api/auth/login")
                .header("Content-Type", "application/json")
                .body(r#"{"email":"jane@example.com","password":"password123"}"#),
        );
        assert_eq!(res.status, Status::TooManyRequests);
    }
}
