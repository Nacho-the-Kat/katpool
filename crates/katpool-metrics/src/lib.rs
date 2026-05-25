//! Prometheus registry helpers used by every service crate.
//!
//! Enforces the operating principle that high-cardinality labels (per-wallet,
//! per-IP) never appear on hot metrics. Helper macros and builders that fail
//! to compile (or at runtime in test mode) if a forbidden cardinality slips in.
//!
//! Implemented in Phase 1 (bridge metrics) and refined in Phase 3 (accountant).

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
