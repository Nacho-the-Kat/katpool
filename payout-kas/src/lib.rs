//! KAS payout engine.
//!
//! Daily cron picks up miners with `balance >= thresholdAmount`, plans a
//! mass-valid set of transactions via [`katpool_storagemass`], signs them
//! using the sops-encrypted treasury key via [`katpool_secrets`], and
//! submits them through the embedded kaspad. Every outbound transaction is
//! recorded with an idempotency key in `idempotency_keys` BEFORE signing
//! so a mid-cycle restart can never double-pay.
//!
//! ## UTXO lifecycle (see `docs/kips.md` §5.4)
//!
//! - **Plan:** `plan_batches` may use virtual change UTXOs to chain many
//!   batches in one offline plan; those outpoints are not broadcastable.
//! - **Execute:** before each sign/submit, refresh treasury UTXOs from
//!   kaspad and bind planned inputs to confirmed coins (prior batch change
//!   replaces the virtual outpoint). Re-run mass check; abort on drift.
//! - **Maintain:** scheduled consolidation when the treasury UTXO count
//!   exceeds threshold (`docs/kips.md` §5.3, runbook 07).
//!
//! Real implementation lands in Phase 4 (M4.3–M4.8).

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
