//! Idempotency keys and distributed-lock primitives.
//!
//! Wraps Postgres concurrency primitives into safe, testable APIs that the
//! payout engines use to guarantee at-most-once side effects across process
//! restarts and concurrent instances.
//!
//! - [`AdvisoryLock`] — single-leader mutual exclusion via a Postgres session
//!   advisory lock, leak-safe on drop (see [`lock`]).
//!
//! Per-recipient payout idempotency itself rests on natural database keys
//! (`payout_cycle.idempotency_key`, `payout UNIQUE (cycle_id, wallet_id)`) in
//! `katpool-db`, not a side table.

#![cfg_attr(not(test), warn(missing_docs))]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod lock;

pub use lock::{AdvisoryLock, advisory_key};

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Shared advisory-lock namespace serializing **all** treasury-spending work.
///
/// Every engine that can build and broadcast a transaction from the treasury
/// (KAS payouts, KRC-20 NACHO payouts, and UTXO consolidation) acquires the
/// lock under this single namespace, so at most one of them spends at a time
/// and they can never select the same UTXO concurrently. Ticks are short and
/// acquired non-blocking via [`AdvisoryLock::try_acquire`]. The payout engines
/// skip a tick outright when another holder has it; consolidation (the
/// lowest-priority spender) instead phase-staggers its poll and waits a bounded
/// interval for the lock, so it always yields to in-flight payouts yet is never
/// starved by them. The trade-off is that the cheap confirm/settle reads also
/// serialize behind it, which is acceptable and removes all cross-engine UTXO
/// contention.
pub const TREASURY_SPEND_LOCK_NAMESPACE: &str = "treasury:spend-leader";

#[cfg(test)]
mod tests {
    use super::{TREASURY_SPEND_LOCK_NAMESPACE, advisory_key};

    #[test]
    fn shared_treasury_lock_key_is_stable_and_pinned() {
        // Stable across calls...
        assert_eq!(
            advisory_key(TREASURY_SPEND_LOCK_NAMESPACE),
            advisory_key(TREASURY_SPEND_LOCK_NAMESPACE)
        );
        // ...and pinned to a golden value so an accidental rename of the
        // namespace (which would silently un-serialize the treasury engines
        // across a rolling deploy) fails the build instead.
        assert_eq!(TREASURY_SPEND_LOCK_NAMESPACE, "treasury:spend-leader");
        assert_eq!(
            advisory_key(TREASURY_SPEND_LOCK_NAMESPACE),
            -2_937_957_302_732_196_380
        );
    }
}
