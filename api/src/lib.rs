//! Public **read-only** HTTP API for the katpool unified runtime (ADR-0021).
//!
//! Embedded in the `katpool` binary as an env-gated task on
//! `KATPOOL_API_PORT`. It exposes three unversioned liveness/readiness probes
//! and a versioned `/api/v1` data surface composed entirely from
//! `katpool-db` repo functions:
//!
//! - `GET /health` / `/ready` / `/started` — liveness / readiness / startup.
//! - `GET /api/v1/pool/{stats,hashrate,hashrate/history,blocks,payouts}`.
//! - `GET /api/v1/balance/{address}`.
//! - `GET /api/v1/miners/{address}`, `.../workers`, `.../hashrate/history`,
//!   `.../payouts`, `.../rejects`.
//! - `GET /api/v1/full_rebate/{address}`.
//!
//! It holds **no funds and no secrets**: it never imports the payout/signing
//! crates and reads `PostgreSQL` only. The edge is per-IP rate-limited
//! (`tower_governor`), body-bounded, hard-timed-out, and TTL-cached
//! (`moka`); on-chain amounts serialize as decimal strings and addresses are
//! redacted in telemetry.

#![cfg_attr(not(test), warn(missing_docs))]

pub mod config;
pub mod error;
pub mod handlers;
pub mod models;
pub mod money;
pub mod params;
pub mod redact;
pub mod state;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::routing::get;
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub use crate::config::ApiConfig;
use crate::config::MAX_BODY_BYTES;
pub use crate::error::ApiError;
pub use crate::state::{AppState, ReadinessHandle};

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How often the background DB readiness probe runs.
const DB_PROBE_INTERVAL: Duration = Duration::from_secs(5);

/// How often the rate-limiter's per-IP storage is garbage-collected.
const GOVERNOR_CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

/// Build the application router with its cheap middleware stack.
///
/// Routes, state, body limit, hard timeout, tracing, and optional CORS. The
/// per-IP rate limiter is **not** applied here — it needs peer-IP connection
/// info and is added by [`serve`]. Exposed for tests, which exercise the
/// router directly via `tower::ServiceExt::oneshot`.
pub fn app(state: AppState) -> Router {
    let config = Arc::clone(&state.config);

    let v1 = Router::new()
        .route("/pool/stats", get(handlers::pool::stats))
        .route("/pool/hashrate", get(handlers::pool::hashrate))
        .route(
            "/pool/hashrate/history",
            get(handlers::pool::hashrate_history),
        )
        .route("/pool/blocks", get(handlers::pool::blocks))
        .route("/pool/payouts", get(handlers::pool::payouts))
        .route("/balance/{address}", get(handlers::miner::balance))
        .route("/miners/{address}", get(handlers::miner::profile))
        .route("/miners/{address}/workers", get(handlers::miner::workers))
        .route(
            "/miners/{address}/hashrate/history",
            get(handlers::miner::hashrate_history),
        )
        .route("/miners/{address}/payouts", get(handlers::miner::payouts))
        .route("/miners/{address}/rejects", get(handlers::miner::rejects))
        .route("/full_rebate/{address}", get(handlers::miner::full_rebate));

    let router = Router::new()
        .route("/health", get(handlers::health::health))
        .route("/ready", get(handlers::health::ready))
        .route("/started", get(handlers::health::started))
        .nest("/api/v1", v1)
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::SERVICE_UNAVAILABLE,
            config.request_timeout,
        ))
        .layer(TraceLayer::new_for_http());

    if let Some(cors) = cors_layer(config.cors_allow_origin.as_deref()) {
        router.layer(cors)
    } else {
        router
    }
}

/// Build a read-only CORS layer for an explicit origin, or `None` to install
/// no CORS layer (same-origin only). A malformed origin disables CORS with a
/// loud log rather than failing startup.
fn cors_layer(origin: Option<&str>) -> Option<CorsLayer> {
    let origin = origin?;
    match origin.parse::<HeaderValue>() {
        Ok(value) => Some(
            CorsLayer::new()
                .allow_methods([Method::GET])
                .allow_headers([header::ACCEPT, header::CONTENT_TYPE])
                .allow_origin(value),
        ),
        Err(err) => {
            tracing::error!(%origin, error = %err, "invalid KATPOOL_API_CORS_ALLOW_ORIGIN; CORS disabled");
            None
        }
    }
}

/// Serve the API on an already-bound listener until the process exits.
///
/// Wraps [`app`] with the per-IP rate limiter and serves with
/// per-connection peer-IP info (required by `PeerIpKeyExtractor`). Spawns a
/// background task to GC the limiter's storage.
///
/// # Errors
/// Propagates any fatal `axum::serve` I/O error.
pub async fn serve(listener: tokio::net::TcpListener, state: AppState) -> std::io::Result<()> {
    let config = Arc::clone(&state.config);
    let router = app(state);

    let governor_conf = GovernorConfigBuilder::default()
        .per_second(config.rate_per_second)
        .burst_size(config.rate_burst)
        .finish();

    let governed = if let Some(conf) = governor_conf {
        let conf = Arc::new(conf);
        // GC the limiter's per-IP storage so memory stays bounded.
        let limiter = conf.limiter().clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(GOVERNOR_CLEANUP_INTERVAL);
            loop {
                ticker.tick().await;
                limiter.retain_recent();
            }
        });
        router.layer(GovernorLayer::new(conf))
    } else {
        tracing::error!("rate-limiter config invalid; serving WITHOUT in-app rate limiting");
        router
    };

    axum::serve(
        listener,
        governed.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}

/// Bind `addr` and serve. Convenience wrapper used by the runtime.
///
/// # Errors
/// Returns the bind error if the address is unavailable, or any fatal serve
/// error thereafter.
pub async fn serve_on(addr: SocketAddr, state: AppState) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "katpool public API listening");
    serve(listener, state).await
}

/// Spawn the background database-reachability probe.
///
/// Probes every `DB_PROBE_INTERVAL` and updates the readiness flag. The
/// runtime owns the kaspad-sync and startup flags (driven by its maturity
/// poller). The returned handle may be dropped; the task runs until abort.
pub fn spawn_db_readiness_probe(pool: PgPool, readiness: ReadinessHandle) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(DB_PROBE_INTERVAL);
        loop {
            ticker.tick().await;
            let ok = sqlx::query("SELECT 1").execute(&pool).await.is_ok();
            readiness.set_db_reachable(ok);
        }
    })
}
