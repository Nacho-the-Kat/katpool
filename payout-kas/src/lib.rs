//! KAS payout engine.
//!
//! Daily cron picks up miners with `balance >= thresholdAmount`, plans a
//! mass-valid set of transactions via [`katpool_storagemass`], signs them
//! using the sops-encrypted treasury key via [`katpool_secrets`], and
//! submits them through the embedded kaspad. Every outbound transaction is
//! recorded with an idempotency key in `idempotency_keys` BEFORE signing
//! so a mid-cycle restart can never double-pay.
//!
//! Real implementation lands in Phase 4.

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
