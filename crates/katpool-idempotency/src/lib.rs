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

#[cfg(test)]
mod tests {
    use super::advisory_key;

    #[test]
    fn advisory_key_is_stable_and_distinct() {
        assert_eq!(
            advisory_key("payout-kas:kas-leader"),
            advisory_key("payout-kas:kas-leader")
        );
        assert_ne!(
            advisory_key("payout-kas:kas-leader"),
            advisory_key("payout-krc20:leader")
        );
    }
}
