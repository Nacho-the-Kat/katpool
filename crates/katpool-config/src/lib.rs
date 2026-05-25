//! Strongly-typed configuration loader for katpool.
//!
//! Loads TOML or YAML config files, validates them at startup via the
//! `validator` crate, and never falls back to silent defaults — a bad
//! config aborts the boot with an actionable error.
//!
//! Implemented in Phase 3 (accountant) and refined in Phase 6 (api).

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
