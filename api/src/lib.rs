//! Public read-only HTTP API.
//!
//! - `/health` — liveness (process is up)
//! - `/ready` — readiness (kaspad synced + DB reachable)
//! - `/started` — startup probe (initial sync complete)
//! - `/balance/:address` — miner balance lookup
//! - `/api/pool/stats` — aggregate pool stats
//! - `/api/pool/hashrate` — current pool hashrate
//! - `/full_rebate/:address` — full-rebate eligibility check
//!
//! No funds, no secrets. Reads from `PostgreSQL` only.
//! Real implementation lands in Phase 6.

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
