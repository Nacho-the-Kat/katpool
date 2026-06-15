//! Per-table transform modules.
//!
//! Each module reads one legacy table, writes into the corresponding
//! new-schema aggregates via the `katpool-db` repo layer, and
//! returns a [`TransformStats`] block for the importer's
//! reconciliation report.

use std::fmt;

pub mod balances;
pub mod blocks;
pub mod krc20;
pub mod nacho_payments;
pub mod payments;

/// Per-transform reconciliation tally. Each transform module emits
/// one of these; the importer sums them into the final report.
#[derive(Debug, Default, Clone)]
pub struct TransformStats {
    /// Rows read from the legacy source.
    pub read: u64,
    /// New rows inserted into the target.
    pub inserted: u64,
    /// Rows already present (idempotent re-run hit).
    pub skipped: u64,
    /// Rows rejected by the importer because their data fails the
    /// new schema's validation (typically wallet-address format).
    pub rejected: u64,
    /// Sum of the monetary field (sompi) over the rejected rows, for the
    /// reconcile's sum-check allowance. `0` for transforms with no sum check.
    pub rejected_amount: i64,
    /// Sum of the monetary field collapsed by the target's
    /// `UNIQUE (cycle_id, wallet_id)` constraint — i.e. legacy rows that are a
    /// *within-cycle* duplicate of an already-credited wallet (run-stable,
    /// detected by a per-cycle seen-set, distinct from cross-run idempotent
    /// re-hits). Feeds the reconcile sum-check allowance. `0` where N/A.
    pub deduped_amount: i64,
}

impl TransformStats {
    /// Sum two stats — for the per-transform → total roll-up.
    #[must_use]
    pub const fn add(&self, other: &Self) -> Self {
        Self {
            read: self.read + other.read,
            inserted: self.inserted + other.inserted,
            skipped: self.skipped + other.skipped,
            rejected: self.rejected + other.rejected,
            rejected_amount: self.rejected_amount + other.rejected_amount,
            deduped_amount: self.deduped_amount + other.deduped_amount,
        }
    }
}

impl fmt::Display for TransformStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "read={} inserted={} skipped={} rejected={} rejected_amount={} deduped_amount={}",
            self.read,
            self.inserted,
            self.skipped,
            self.rejected,
            self.rejected_amount,
            self.deduped_amount
        )
    }
}
