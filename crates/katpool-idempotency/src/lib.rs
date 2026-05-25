//! Idempotency keys and distributed-lock primitives.
//!
//! Wraps the `idempotency_keys` and `distributed_locks` tables defined in
//! `katpool-db` (Phase 2) into safe, testable APIs that the accountant and
//! payout engines use to guarantee at-most-once side effects across process
//! restarts and concurrent instances.
//!
//! Real implementation lands in Phase 2 (table) and Phase 4 (consumer).

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
