// Copyright 2026 Internet2
// SPDX-License-Identifier: Apache-2.0

//! Serves a catalog snapshot over HTTP.
//!
//! Locally it binds a port (and can serve the `web/` front end alongside, for one-
//! command development). Under AWS Lambda — detected by `AWS_LAMBDA_RUNTIME_API` — the
//! same router runs behind API Gateway via `lambda_http`.
//!
//! Environment:
//! - `CATALOG_SOURCE` — snapshot location: path, `s3://bucket/key`, or `https://…`
//! - `CATALOG_REFRESH_SECS` — how often to check for a newer snapshot (default 300)
//! - `CORS_ALLOW_ORIGIN` — allowed origin for browsers (default `*`; the data is public)
//! - `PORT`, `BIND_ADDR` (default `127.0.0.1`; the container sets `0.0.0.0`),
//!   `CATALOG_STATIC_DIR` — local mode only
//! - natural-language search: see `nl.rs` (`ANTHROPIC_API_KEY`, `CATALOG_NL_*`)

mod nl;
mod releases;
mod routes;
mod state;

use std::time::Duration;

use anyhow::Result;
use axum::http::{HeaderValue, Method};
use catalog_store::Location;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .without_time()
        .init();

    let source = std::env::var("CATALOG_SOURCE").unwrap_or_else(|_| "data/catalog.json.gz".into());
    let refresh = std::env::var("CATALOG_REFRESH_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    let reader = nl::ClaudeReader::from_env().await?;
    match &reader {
        Some(r) => tracing::info!(
            model = r.model(),
            "natural-language search: Claude, with rules as fallback"
        ),
        None => {
            tracing::info!("natural-language search: built-in rules (no Anthropic key configured)")
        }
    }
    let state = state::AppState::load(
        Location::parse(&source)?,
        Duration::from_secs(refresh),
        reader,
    )
    .await?;

    let cors = match std::env::var("CORS_ALLOW_ORIGIN").ok().filter(|o| o != "*") {
        Some(origins) => AllowOrigin::list(
            origins
                .split(',')
                .filter_map(|o| HeaderValue::from_str(o.trim()).ok()),
        ),
        None => AllowOrigin::any(),
    };
    let mut app = routes::router(state).layer(
        CorsLayer::new()
            .allow_origin(cors)
            .allow_methods([Method::GET, Method::HEAD]),
    );

    if std::env::var_os("AWS_LAMBDA_RUNTIME_API").is_some() {
        // API Gateway compresses nothing for us, but lambda_http base64-encodes any
        // body with a Content-Encoding, so gzip is safe here.
        app = app.layer(CompressionLayer::new());
        return lambda_http::run(app).await.map_err(|e| anyhow::anyhow!(e));
    }

    if let Ok(dir) = std::env::var("CATALOG_STATIC_DIR") {
        // Revalidate every load, so an edited app.js is never served stale.
        app = app.fallback_service(tower_http::set_header::SetResponseHeader::overriding(
            tower_http::services::ServeDir::new(dir),
            axum::http::header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ));
    }
    let app = app
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1".into());
    let listener = tokio::net::TcpListener::bind(format!("{addr}:{port}")).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
