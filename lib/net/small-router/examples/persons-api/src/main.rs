/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A simple persons REST API example

use std::env;
use std::net::{Ipv4Addr, TcpListener};

use bsql::{Connection, MysqlTransport, PoolOptions, SqliteMode};
use log::info;
use small_router::{Router, RouterBuilder};

use crate::context::Context;
use crate::controllers::{
    home, not_found, persons_create, persons_delete, persons_index, persons_show, persons_update,
};

mod api {
    include!(concat!(env!("OUT_DIR"), "/persons_api.rs"));
}
mod context;
mod controllers;
mod layers;
mod models;
mod validators;

fn router(ctx: Context) -> Router<Context> {
    RouterBuilder::<Context>::with(ctx)
        .pre_layer(layers::log_pre_layer)
        .pre_layer(layers::cors_pre_layer)
        .post_layer(layers::cors_post_layer)
        .get("/", home)
        .get("/persons", persons_index)
        .post("/persons", persons_create)
        .get("/persons/:person_id", persons_show)
        .put("/persons/:person_id", persons_update)
        .delete("/persons/:person_id", persons_delete)
        .fallback(not_found)
        .build()
}

fn open_database(pool_options: PoolOptions) -> anyhow::Result<Connection> {
    match env::var("DATABASE_DRIVER").as_deref().unwrap_or("sqlite") {
        "sqlite" => Ok(Connection::open_sqlite(
            env::var("DATABASE_PATH").unwrap_or_else(|_| "database.db".to_string()),
            SqliteMode::ReadWrite,
            pool_options,
        )?),
        "mysql" => {
            let host = env::var("DATABASE_HOST").unwrap_or_else(|_| "localhost".to_string());
            let port = env::var("DATABASE_PORT")
                .unwrap_or_else(|_| "3306".to_string())
                .parse()?;
            Ok(Connection::open_mysql(
                MysqlTransport::tcp(host, port, false),
                env::var("DATABASE_USER").unwrap_or_else(|_| "root".to_string()),
                env::var("DATABASE_PASSWORD").unwrap_or_default(),
                Some(&env::var("DATABASE_NAME").unwrap_or_else(|_| "persons".to_string())),
                pool_options,
            )?)
        }
        driver => anyhow::bail!("Unsupported DATABASE_DRIVER: {driver}"),
    }
}

fn main() {
    // Init logger
    if env::var("GATEWAY_INTERFACE").is_err() {
        simple_logger::init().expect("Failed to init logger");
    }

    // Load environment variables
    _ = dotenvy::dotenv();

    // Create router and load database
    let is_cgi = env::var("GATEWAY_INTERFACE").is_ok();
    let pool_options = if is_cgi {
        PoolOptions::single_connection()
    } else {
        PoolOptions::default()
    };
    let context = open_database(pool_options)
        .and_then(Context::with_database)
        .expect("Can't open/create database");
    let router = router(context);

    if is_cgi {
        small_http::serve_cgi(move |req| router.handle(req));
    } else {
        // Start server
        let http_port = env::var("SERVER_PORT")
            .ok()
            .and_then(|port| port.parse::<u16>().ok())
            .unwrap_or(8080);
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, http_port))
            .unwrap_or_else(|_| panic!("Can't bind to port: {http_port}"));
        info!("Server is listening on: http://localhost:{http_port}/");

        small_http::serve(listener, move |req| router.handle(req));
    }
}
