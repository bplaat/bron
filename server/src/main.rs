//! Serves the website: the static files Vite builds into dist/. Environment: PORT (8080) and DIST_PATH (dist).

use std::net::Ipv4Addr;
use std::{env, io};

use axum::Router;
use axum::http::{HeaderValue, header};
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

#[tokio::main]
async fn main() -> io::Result<()> {
    let port = env::var("PORT").ok().and_then(|port| port.parse().ok()).unwrap_or(8080);
    let dist = env::var("DIST_PATH").unwrap_or_else(|_| "dist".to_string());

    // Vite's hashed assets never change, everything else is checked with its Last-Modified date on every load.
    // Compression is the reverse proxy's job (see README.md), the disk chunks and snapshot are zstd already.
    let assets = Router::new()
        .fallback_service(ServeDir::new(format!("{dist}/assets")))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ));
    let app = Router::new()
        .nest("/assets", assets)
        .fallback_service(ServeDir::new(&dist))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ));

    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await?;
    println!("Serving {dist} on http://localhost:{port}");
    axum::serve(listener, app).with_graceful_shutdown(shutdown()).await
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
