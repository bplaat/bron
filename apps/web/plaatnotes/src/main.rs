/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::env;
use std::net::{Ipv4Addr, TcpListener};
use std::process::exit;

use log::info;
use small_router::{Router, RouterBuilder};

use crate::args::{Subcommand, parse_args, subcommand_help};
use crate::context::Context;
use crate::controllers::*;

mod api {
    include!(concat!(env!("OUT_DIR"), "/api.rs"));
}
mod args;
mod consts;
mod context;
mod controllers;
mod imports;
mod layers;
mod models;
mod tasks;
#[cfg(test)]
mod test_utils;
mod utils;

pub(crate) fn router(ctx: Context) -> Router<Context> {
    // Guests routes
    let router = RouterBuilder::<Context>::with(ctx)
        .pre_layer(layers::log_pre_layer)
        .pre_layer(layers::cors_pre_layer)
        .post_layer(layers::cors_post_layer)
        .pre_layer(layers::auth_optional_pre_layer)
        .get("/api", home)
        // Auth
        .post("/api/auth/login", auth_login)
        .pre_layer(layers::spa_file_server_pre_layer);

    // Authed routes
    router
        .pre_layer(layers::auth_required_pre_layer)
        // Auth
        .get("/api/auth/validate", auth_validate)
        .post("/api/auth/logout", auth_logout)
        // Users
        .get("/api/users", users_index)
        .post("/api/users", users_create)
        .get("/api/users/:user_id", users_show)
        .put("/api/users/:user_id", users_update)
        .delete("/api/users/:user_id", users_delete)
        .post("/api/users/:user_id/change-password", users_change_password)
        .post("/api/users/:user_id/login", users_login)
        .get("/api/users/:user_id/notes", users_notes)
        .post("/api/users/:user_id/notes", users_notes_create)
        .put("/api/users/:user_id/notes/reorder", users_notes_reorder)
        .get("/api/users/:user_id/notes/pinned", users_notes_pinned)
        .put(
            "/api/users/:user_id/notes/pinned/reorder",
            users_notes_pinned_reorder,
        )
        .get("/api/users/:user_id/notes/archived", users_notes_archived)
        .put(
            "/api/users/:user_id/notes/archived/reorder",
            users_notes_archived_reorder,
        )
        .get("/api/users/:user_id/notes/trashed", users_notes_trashed)
        .delete(
            "/api/users/:user_id/notes/trashed/clear",
            users_notes_trashed_clear,
        )
        .get("/api/users/:user_id/sessions", users_sessions)
        // Sessions
        .get("/api/sessions", sessions_index)
        .get("/api/sessions/:session_id", sessions_show)
        .delete("/api/sessions/:session_id", sessions_delete)
        // Notes
        .get("/api/notes", notes_index)
        .post("/api/notes", notes_create)
        .get("/api/notes/:note_id", notes_show)
        .put("/api/notes/:note_id", notes_update)
        .delete("/api/notes/:note_id", notes_delete)
        // Imports
        .post("/api/imports/google-keep", imports_google_keep)
        .build()
}

fn main() {
    // Load environment variables
    _ = dotenvy::dotenv();

    // Parse args
    let args = parse_args();
    if args.subcommand == Subcommand::Help {
        subcommand_help();
        return;
    }
    if args.subcommand == Subcommand::Version {
        println!("plaatnotes v{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Init logger
    simple_logger::init().expect("Failed to init logger");

    // Init context
    let data_path = env::var("DATA_PATH").unwrap_or_else(|_| ".".to_string());
    if let Err(e) = std::fs::create_dir_all(&data_path) {
        log::warn!("Could not create data directory {data_path}: {e}");
    }

    let context = if args.subcommand == Subcommand::ServeE2e {
        use crate::context::DatabaseHelpers;
        use crate::models::{User, UserRole};
        use crate::utils::password_hash;
        let ctx = Context::with_e2e_database().expect("Can't create E2E test database");
        ctx.database
            .insert_user(User {
                first_name: "Test".to_string(),
                last_name: "User".to_string(),
                email: "test@example.com".to_string(),
                password: password_hash("password"),
                role: UserRole::Normal,
                ..Default::default()
            })
            .expect("Database error");
        ctx.database
            .insert_user(User {
                first_name: "Test".to_string(),
                last_name: "Admin".to_string(),
                email: "testadmin@example.com".to_string(),
                password: password_hash("password"),
                role: UserRole::Admin,
                ..Default::default()
            })
            .expect("Database error");
        ctx
    } else {
        Context::with_database(
            format!("{data_path}/database.db"),
            env::var("SERVER_ORIGIN").unwrap_or_else(|_| "*".to_string()),
        )
        .expect("Can't open/create database")
    };

    // Start task runner (also handles DB-IP database download on startup)
    let mmdb_path = format!("{data_path}/dbip-city-lite.mmdb");
    tasks::start_task_runner(context.clone(), mmdb_path);

    if args.subcommand == Subcommand::ImportGoogleKeep {
        let path = args.path.unwrap_or_else(|| {
            eprintln!("Usage: plaatnotes import-google-keep <path> --email <email>");
            exit(1);
        });
        let email = args.email.unwrap_or_else(|| {
            eprintln!("Error: --email <email> is required for import-google-keep");
            exit(1);
        });
        imports::google_keep::run(&path, &email, &context)
            .unwrap_or_else(|error| panic!("Google Keep import failed: {error}"));
        return;
    }

    // Start server
    let http_port = env::var("SERVER_PORT")
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(8080);
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, http_port))
        .unwrap_or_else(|_| panic!("Can't bind to port: {http_port}"));
    info!("Server is listening on: http://localhost:{http_port}/");

    let router = router(context);
    small_http::serve(listener, move |req| router.handle(req));
}
