//! Core domain types for katpool.
//!
//! This crate is the lowest layer in the workspace. It contains only pure,
//! deterministic types — no I/O, no async, no global state. Every other crate
//! depends on it. Adding a new type here is a deliberate decision: if the type
//! has external dependencies (database, network, filesystem) it does not
//! belong here.
//!
//! Types added in subsequent phases:
//! - Phase 1: [`WalletAddress`], [`WorkerName`], [`ShareDifficulty`],
//!   [`BlockTemplate`], [`Sompi`], [`NachoUnits`], [`IdempotencyKey`].
//! - Phase 3: [`ShareWindow`], reward allocation types.
//! - Phase 4: storage-mass-related types live in `katpool-storagemass`, not here.

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant, useful for diagnostic reporting and logging.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
