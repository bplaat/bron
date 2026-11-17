//! Serves the website: the static files Vite builds into web/dist/ and the LAN relay between the visitors' Windows XP
//! desktops on /relay. Environment: PORT (8080), DIST_PATH (web/dist), ORIGIN and TRUST_PROXY, see relay.rs.

use std::net::{Ipv4Addr, SocketAddr};
use std::{env, io};

use axum::Router;
use axum::http::{HeaderValue, header};
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

mod relay;

#[tokio::main]
async fn main() -> io::Result<()> {
    let port = env::var("PORT").ok().and_then(|port| port.parse().ok()).unwrap_or(8080);
    let dist = env::var("DIST_PATH").unwrap_or_else(|_| "web/dist".to_string());

    // Vite's hashed assets never change, everything else is checked with its Last-Modified date on every load.
    // Compression is the reverse proxy's job (see README.md), the disk chunks and snapshot are zstd already.
    let assets = Router::new()
        .fallback_service(ServeDir::new(format!("{dist}/assets")))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ));
    let app = Router::new()
        .route("/relay", get(relay::handler))
        .nest("/assets", assets)
        .fallback_service(ServeDir::new(&dist))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
        .with_state(relay::Relay::spawn(relay::Config::from_env()));

    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
    println!("Serving {dist} on http://localhost:{port}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown())
        .await
}

// Ctrl+C, or SIGTERM from docker stop
async fn shutdown() {
    let interrupt = async {
        signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut terminate) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
            terminate.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }
}
