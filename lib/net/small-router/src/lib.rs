/*
 * Copyright (c) 2024-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::cmp::Ordering;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use anyhow::Result;
use percent_encoding::percent_decode_str;
use small_http::{Method, Request, Response, Status};

// MARK: Handler

// Parsed path parameters
type HandlerFn<T> = Arc<dyn Fn(&Request, &T) -> Result<Response> + Send + Sync>;
type PreLayerFn<T> = Arc<dyn Fn(&Request, &mut T) -> Result<Option<Response>> + Send + Sync>;
type PostLayerFn<T> = Arc<dyn Fn(&Request, &T, Response) -> Result<Response> + Send + Sync>;
type ErrorHandlerFn<T> = Arc<dyn Fn(&Request, &T, &dyn Error) -> Response + Send + Sync>;

struct Handler<T> {
    handler: HandlerFn<T>,
    pre_layers: Vec<PreLayerFn<T>>,
    post_layers: Vec<PostLayerFn<T>>,
}

impl<T> Handler<T> {
    fn new(
        handler: HandlerFn<T>,
        pre_layers: Vec<PreLayerFn<T>>,
        post_layers: Vec<PostLayerFn<T>>,
    ) -> Self {
        Self {
            handler,
            pre_layers,
            post_layers,
        }
    }

    fn call(&self, req: &Request, ctx: &mut T) -> Result<Response> {
        for pre_layer in &self.pre_layers {
            if let Some(mut res) = pre_layer(req, ctx)? {
                for post_layer in &self.post_layers {
                    res = post_layer(req, ctx, res)?;
                }
                return Ok(res);
            }
        }
        let mut res = (self.handler)(req, ctx)?;
        for post_layer in &self.post_layers {
            res = post_layer(req, ctx, res)?;
        }
        Ok(res)
    }
}

// MARK: Route
enum RoutePart {
    Static(String),
    Param(String),
}

struct Route<T> {
    methods: Vec<Method>,
    parts: Vec<RoutePart>,
    handler: Handler<T>,
}

impl<T> Route<T> {
    fn new(methods: Vec<Method>, route: String, handler: Handler<T>) -> Self {
        let parts = Self::route_parse_parts(&route);
        Self {
            methods,
            parts,
            handler,
        }
    }

    fn route_parse_parts(route: &str) -> Vec<RoutePart> {
        route
            .split('/')
            .filter(|part| !part.is_empty())
            .map(|part| {
                if let Some(stripped) = part.strip_prefix(':') {
                    RoutePart::Param(stripped.to_string())
                } else {
                    RoutePart::Static(part.to_string())
                }
            })
            .collect()
    }

    /// Match a request path, returning the percent-decoded parameters
    fn match_path(&self, path: &str) -> Option<HashMap<String, String>> {
        let mut path_parts = path.split('/').filter(|part| !part.is_empty());
        let mut params = HashMap::new();
        for part in &self.parts {
            let actual = path_parts.next()?;
            match part {
                RoutePart::Static(expected) => {
                    if actual != expected {
                        return None;
                    }
                }
                RoutePart::Param(name) => {
                    let value = percent_decode_str(actual).decode_utf8_lossy();
                    params.insert(name.clone(), value.into_owned());
                }
            }
        }
        path_parts.next().is_none().then_some(params)
    }

    fn has_same_shape(&self, other: &Self) -> bool {
        self.parts.len() == other.parts.len()
            && self
                .parts
                .iter()
                .zip(&other.parts)
                .all(|(left, right)| match (left, right) {
                    (RoutePart::Static(left), RoutePart::Static(right)) => left == right,
                    (RoutePart::Param(_), RoutePart::Param(_)) => true,
                    _ => false,
                })
    }
}

// MARK: RouterBuilder
/// Router builder
pub struct RouterBuilder<T: Clone> {
    ctx: T,
    pre_layers: Vec<PreLayerFn<T>>,
    post_layers: Vec<PostLayerFn<T>>,
    routes: Vec<Route<T>>,
    not_allowed_method_handler: Option<Handler<T>>,
    fallback_handler: Option<Handler<T>>,
    error_handler: Option<ErrorHandlerFn<T>>,
}

impl Default for RouterBuilder<()> {
    fn default() -> Self {
        Self::with(())
    }
}

impl RouterBuilder<()> {
    /// Create new router
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T: Clone> RouterBuilder<T> {
    /// Create new router with context
    pub fn with(ctx: T) -> Self {
        Self {
            ctx,
            pre_layers: Vec::new(),
            post_layers: Vec::new(),
            routes: Vec::new(),
            not_allowed_method_handler: None,
            fallback_handler: None,
            error_handler: None,
        }
    }

    /// Add pre layer
    pub fn pre_layer(
        mut self,
        layer: impl Fn(&Request, &mut T) -> Result<Option<Response>> + Send + Sync + 'static,
    ) -> Self {
        self.pre_layers.push(Arc::new(layer));
        self
    }

    /// Add post layer
    pub fn post_layer(
        mut self,
        layer: impl Fn(&Request, &T, Response) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.post_layers.push(Arc::new(layer));
        self
    }

    /// Add route
    pub fn route(
        mut self,
        methods: &[Method],
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.routes.push(Route::new(
            methods.to_vec(),
            route.into(),
            Handler::new(
                Arc::new(handler),
                self.pre_layers.clone(),
                self.post_layers.clone(),
            ),
        ));
        self
    }

    /// Add route for any method
    pub fn any(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(
            &[
                Method::Get,
                Method::Head,
                Method::Post,
                Method::Put,
                Method::Delete,
                Method::Connect,
                Method::Options,
                Method::Trace,
                Method::Patch,
            ],
            route,
            handler,
        )
    }
    /// Add route for GET method
    pub fn get(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Get], route, handler)
    }

    /// Add route for HEAD method
    pub fn head(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Head], route, handler)
    }

    /// Add route for POST method
    pub fn post(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Post], route, handler)
    }

    /// Add route for PUT method
    pub fn put(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Put], route, handler)
    }

    /// Add route for DELETE method
    pub fn delete(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Delete], route, handler)
    }

    /// Add route for CONNECT method
    pub fn connect(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Connect], route, handler)
    }

    /// Add route for OPTIONS method
    pub fn options(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Options], route, handler)
    }

    /// Add route for TRACE method
    pub fn trace(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Trace], route, handler)
    }

    /// Add route for PATCH method
    pub fn patch(
        self,
        route: impl Into<String>,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.route(&[Method::Patch], route, handler)
    }

    /// Set not allowed method handler (called when a route matches but method doesn't)
    pub fn not_allowed_method(
        mut self,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.not_allowed_method_handler = Some(Handler::new(
            Arc::new(handler),
            self.pre_layers.clone(),
            self.post_layers.clone(),
        ));
        self
    }

    /// Set fallback handler
    pub fn fallback(
        mut self,
        handler: impl Fn(&Request, &T) -> Result<Response> + Send + Sync + 'static,
    ) -> Self {
        self.fallback_handler = Some(Handler::new(
            Arc::new(handler),
            self.pre_layers.clone(),
            self.post_layers.clone(),
        ));
        self
    }

    /// Set error handler (called when a handler returns an error)
    pub fn error(
        mut self,
        handler: impl Fn(&Request, &T, &dyn Error) -> Response + Send + Sync + 'static,
    ) -> Self {
        self.error_handler = Some(Arc::new(handler));
        self
    }

    /// Build router
    pub fn build(self) -> Router<T> {
        // Sort routes: longest first, then prefer static parts over params at each position
        let mut routes = self.routes;
        routes.sort_by(|a, b| {
            let len_cmp = b.parts.len().cmp(&a.parts.len());
            if len_cmp != Ordering::Equal {
                return len_cmp;
            }
            for (pa, pb) in a.parts.iter().zip(b.parts.iter()) {
                let score = |p: &RoutePart| matches!(p, RoutePart::Static(_)) as u8;
                let part_cmp = score(pb).cmp(&score(pa));
                if part_cmp != Ordering::Equal {
                    return part_cmp;
                }
            }
            Ordering::Equal
        });

        Router(Arc::new(InnerRouter {
            ctx: self.ctx,
            routes,
            not_allowed_method_handler: self.not_allowed_method_handler.unwrap_or_else(|| {
                Handler::new(
                    Arc::new(|_, _| {
                        Ok(Response::with_status(Status::MethodNotAllowed)
                            .body("405 Method Not Allowed"))
                    }),
                    self.pre_layers.clone(),
                    self.post_layers.clone(),
                )
            }),
            fallback_handler: self.fallback_handler.unwrap_or_else(|| {
                Handler::new(
                    Arc::new(|_, _| {
                        Ok(Response::with_status(Status::NotFound).body("404 Not Found"))
                    }),
                    self.pre_layers.clone(),
                    self.post_layers.clone(),
                )
            }),
            error_handler: self.error_handler.unwrap_or_else(|| {
                Arc::new(|req: &Request, _: &T, err: &dyn Error| {
                    cfg_select! {
                        feature = "log" => {
                            log::error!("Handling request {} {}: {}", req.method, req.url, err)
                        }
                        _ => eprintln!(
                            "[small-router] Error handling request {} {}: {}",
                            req.method, req.url, err
                        ),
                    }
                    Response::with_status(Status::InternalServerError)
                        .body("500 Internal Server Error")
                })
            }),
        }))
    }
}

// MARK: InnerRouter
struct InnerRouter<T: Clone> {
    ctx: T,
    routes: Vec<Route<T>>,
    not_allowed_method_handler: Handler<T>,
    fallback_handler: Handler<T>,
    error_handler: ErrorHandlerFn<T>,
}

impl<T: Clone> InnerRouter<T> {
    fn handle(&self, req: &Request) -> Response {
        let mut ctx = self.ctx.clone();
        match self.handle_inner(req, &mut ctx) {
            Ok(res) => res,
            Err(err) => (self.error_handler)(req, &mut ctx, &*err),
        }
    }

    fn handle_inner(&self, req: &Request, ctx: &mut T) -> Result<Response> {
        // Match routes
        let path = req.url.path();
        if let Some((first_match, params)) = self
            .routes
            .iter()
            .find_map(|route| Some((route, route.match_path(path)?)))
        {
            // Find a matching method among routes with the same parsed path shape. Parameter names
            // may differ between method-specific route registrations.
            let mut req = req.clone();
            if let Some(route) = self.routes.iter().find(|route| {
                route.has_same_shape(first_match) && route.methods.contains(&req.method)
            }) {
                req.params = if std::ptr::eq(route, first_match) {
                    params
                } else {
                    route.match_path(path).unwrap_or_default()
                };
                return route.handler.call(&req, ctx);
            }

            // Or run not allowed method handler
            req.params = params;
            return self.not_allowed_method_handler.call(&req, ctx);
        }

        // Or run fallback handler
        self.fallback_handler.call(req, ctx)
    }
}

// MARK: Router
/// Router
#[derive(Clone)]
pub struct Router<T: Clone>(Arc<InnerRouter<T>>);

impl<T: Clone> Router<T> {
    /// Handle request
    pub fn handle(&self, req: &Request) -> Response {
        self.0.handle(req)
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use small_http::Status;

    use super::*;

    fn home(_req: &Request, _ctx: &()) -> Result<Response> {
        Ok(Response::with_status(Status::Ok).body("Hello, World!"))
    }

    fn hello(req: &Request, _ctx: &()) -> Result<Response> {
        let name = req.params.get("name").unwrap();
        Ok(Response::with_status(Status::Ok).body(format!("Hello, {name}!")))
    }

    fn error(_req: &Request, _ctx: &()) -> Result<Response> {
        Err(anyhow::anyhow!("Test error"))
    }

    #[test]
    fn test_routing() {
        let router = RouterBuilder::new()
            .get("/", home)
            .get("/hello/:name", hello)
            .get("/hello/:name/i/:am/so/:deep", hello)
            .get("/error", error)
            .build();

        // Test home route
        let res = router.handle(&Request::get("http://localhost/"));
        assert_eq!(res.status, Status::Ok);
        assert_eq!(res.body, b"Hello, World!");

        // Test fallback route
        let res = router.handle(&Request::get("http://localhost/unknown"));
        assert_eq!(res.status, Status::NotFound);
        assert_eq!(res.body, b"404 Not Found");

        // Test route with params
        let res = router.handle(&Request::get("http://localhost/hello/Bassie"));
        assert_eq!(res.status, Status::Ok);
        assert_eq!(res.body, b"Hello, Bassie!");

        // Test route with multiple params
        let res = router.handle(&Request::get(
            "http://localhost/hello/Bassie/i/handle/so/much",
        ));
        assert_eq!(res.status, Status::Ok);

        // Test wrong method
        let res = router.handle(&Request::options("http://localhost/"));
        assert_eq!(res.status, Status::MethodNotAllowed);
        assert_eq!(res.body, b"405 Method Not Allowed");

        // Test error route
        let res = router.handle(&Request::get("http://localhost/error"));
        assert_eq!(res.status, Status::InternalServerError);
        assert_eq!(res.body, b"500 Internal Server Error");
    }

    #[test]
    fn test_capturing_closures_and_pre_layer_errors() {
        let pre_layer_calls = Arc::new(AtomicUsize::new(0));
        let calls = pre_layer_calls.clone();
        let blocked_path = "/blocked".to_string();
        let greeting = "Hello from a closure".to_string();
        let error_prefix = "Layer failed".to_string();

        let router = RouterBuilder::new()
            .pre_layer(move |req, _| {
                calls.fetch_add(1, Ordering::Relaxed);
                if req.url.path() == blocked_path {
                    anyhow::bail!("blocked by pre-layer");
                }
                Ok(None)
            })
            .get("/", move |_, _| Ok(Response::with_body(greeting.clone())))
            .get("/blocked", |_, _| Ok(Response::new()))
            .error(move |_, _, err| {
                Response::with_status(Status::InternalServerError)
                    .body(format!("{error_prefix}: {err}"))
            })
            .build();

        let res = router.handle(&Request::get("http://localhost/"));
        assert_eq!(res.status, Status::Ok);
        assert_eq!(res.body, b"Hello from a closure");

        let res = router.handle(&Request::get("http://localhost/blocked"));
        assert_eq!(res.status, Status::InternalServerError);
        assert_eq!(res.body, b"Layer failed: blocked by pre-layer");
        assert_eq!(pre_layer_calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_equivalent_route_shapes_support_different_parameter_names() {
        let router = RouterBuilder::new()
            .get("/users/:id", |req, _| {
                Ok(Response::with_body(req.params["id"].clone()))
            })
            .post("/users/:user_id", |req, _| {
                Ok(Response::with_body(req.params["user_id"].clone()))
            })
            .build();

        let get = router.handle(&Request::get("http://localhost/users/10"));
        assert_eq!(get.status, Status::Ok);
        assert_eq!(get.body, b"10");

        let post = router.handle(&Request::post("http://localhost/users/20"));
        assert_eq!(post.status, Status::Ok);
        assert_eq!(post.body, b"20");
    }

    #[test]
    fn test_parameters_are_percent_decoded() {
        let router = RouterBuilder::new().get("/hello/:name", hello).build();
        let res = router.handle(&Request::get(
            "http://localhost/hello/Bassie%20van%20der%20Plaat",
        ));
        assert_eq!(res.body, b"Hello, Bassie van der Plaat!");
        let res = router.handle(&Request::get("http://localhost/hello/caf%C3%A9"));
        assert_eq!(res.body, "Hello, café!".as_bytes());
    }

    #[test]
    fn test_static_route_keeps_precedence_over_parameter_route() {
        let router = RouterBuilder::new()
            .get("/users/current", |_, _| Ok(Response::with_body("current")))
            .post("/users/:id", |_, _| Ok(Response::with_body("parameter")))
            .build();

        let response = router.handle(&Request::post("http://localhost/users/current"));
        assert_eq!(response.status, Status::MethodNotAllowed);
    }
}
