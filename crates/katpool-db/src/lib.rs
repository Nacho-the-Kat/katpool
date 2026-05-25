//! `PostgreSQL` access layer for katpool.
//!
//! Owns the schema (via sqlx migrations under `migrations/`) and the
//! repository traits that the service crates depend on. Every public
//! function returns a typed error from `DbError`; the service crates
//! never see raw `sqlx::Error`.
//!
//! Real schema and repos land in Phase 2.

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
