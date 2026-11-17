/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::sync::LazyLock;
use std::time::Duration;

use jiff::Timestamp;

use crate::consts::SESSION_EXPIRY_SECONDS;
use crate::context::{Context, DatabaseHelpers};
use crate::models::{Note, Session, User, UserRole};
use crate::utils::password_hash;

pub(crate) static TEST_PASSWORD_HASH: LazyLock<String> =
    LazyLock::new(|| password_hash("password123"));

// Creates a test user and returns (user, token)
pub(crate) fn create_test_user_with_session(ctx: &Context) -> (User, String) {
    create_test_user_with_session_and_role(ctx, UserRole::Normal)
}

// Creates a test user with specific role and returns (user, token)
pub(crate) fn create_test_user_with_session_and_role(
    ctx: &Context,
    role: UserRole,
) -> (User, String) {
    // Create test user with specified role
    let user = User {
        first_name: "Test".to_string(),
        last_name: "User".to_string(),
        email: format!("test-{}@example.com", uuid::Uuid::now_v7()),
        password: TEST_PASSWORD_HASH.to_string(),
        role,
        ..Default::default()
    };
    ctx.database
        .insert_user(user.clone())
        .expect("Database error");

    // Create session with token
    let token = format!("test-token-{}", user.id);
    let session = Session {
        user_id: user.id,
        token: token.clone(),
        expires_at: Timestamp::now() + Duration::from_secs(SESSION_EXPIRY_SECONDS),
        ..Default::default()
    };
    ctx.database
        .insert_session(session)
        .expect("Database error");

    (user, token)
}

pub(crate) fn insert_test_user(
    ctx: &Context,
    first_name: &str,
    last_name: &str,
    email: &str,
) -> User {
    let user = User {
        first_name: first_name.to_string(),
        last_name: last_name.to_string(),
        email: email.to_string(),
        password: TEST_PASSWORD_HASH.to_string(),
        ..Default::default()
    };
    ctx.database
        .insert_user(user.clone())
        .expect("Database error");
    user
}

pub(crate) fn insert_test_session(ctx: &Context, user_id: uuid::Uuid, token: &str) -> Session {
    let session = Session {
        user_id,
        token: token.to_string(),
        expires_at: Timestamp::now() + Duration::from_secs(SESSION_EXPIRY_SECONDS),
        ..Default::default()
    };
    ctx.database
        .insert_session(session.clone())
        .expect("Database error");
    session
}

pub(crate) fn insert_test_note(
    ctx: &Context,
    user_id: uuid::Uuid,
    title: Option<&str>,
    body: &str,
) -> Note {
    let note = Note {
        user_id,
        title: title.map(|t| t.to_string()),
        body: body.to_string(),
        ..Default::default()
    };
    ctx.database
        .insert_note(note.clone())
        .expect("Database error");
    note
}
