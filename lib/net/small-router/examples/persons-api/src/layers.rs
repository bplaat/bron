/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use anyhow::Result;
use log::info;
use small_http::{Method, Request, Response, Status};

use crate::context::Context;

// MARK: Log
pub(crate) fn log_pre_layer(req: &Request, _: &mut Context) -> Result<Option<Response>> {
    info!("{} {}", req.method, req.url.path());
    Ok(None)
}

// MARK: CORS
pub(crate) fn cors_pre_layer(req: &Request, _: &mut Context) -> Result<Option<Response>> {
    if req.method == Method::Options && req.headers.get("Access-Control-Request-Method").is_some() {
        Ok(Some(
            Response::with_status(Status::NoContent)
                .header("Access-Control-Allow-Origin", "*")
                .header(
                    "Access-Control-Allow-Methods",
                    "GET, POST, PUT, PATCH, DELETE, OPTIONS",
                )
                .header("Access-Control-Allow-Headers", "Content-Type")
                .header("Access-Control-Max-Age", "86400"),
        ))
    } else {
        Ok(None)
    }
}

pub(crate) fn cors_post_layer(req: &Request, _: &Context, res: Response) -> Result<Response> {
    if !(req.method == Method::Options
        && req.headers.get("Access-Control-Request-Method").is_some())
    {
        Ok(res.header("Access-Control-Allow-Origin", "*"))
    } else {
        Ok(res)
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;
    use crate::router;

    #[test]
    fn test_cors() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        let res = router.handle(Request::get("http://localhost/"));
        assert_eq!(res.headers.get("Access-Control-Allow-Origin"), Some("*"));
    }

    #[test]
    fn test_cors_preflight() {
        let ctx = Context::with_test_database().expect("Can't create test database");
        let router = router(ctx.clone());

        let res = router.handle(
            Request::options("http://localhost/").header("Access-Control-Request-Method", "POST"),
        );
        assert_eq!(res.headers.get("Access-Control-Allow-Origin"), Some("*"));
        assert_eq!(
            res.headers.get("Access-Control-Allow-Methods"),
            Some("GET, POST, PUT, PATCH, DELETE, OPTIONS")
        );
        assert_eq!(
            res.headers.get("Access-Control-Allow-Headers"),
            Some("Content-Type")
        );
        assert_eq!(res.headers.get("Access-Control-Max-Age"), Some("86400"));
    }
}
