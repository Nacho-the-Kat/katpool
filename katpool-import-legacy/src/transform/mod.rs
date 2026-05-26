//! Per-table transform modules.
//!
//! Each module reads one legacy table, writes into the corresponding
//! new-schema aggregates via the `katpool-db` repo layer, and
//! returns a [`TransformStats`] block for the importer's
//! reconciliation report.

use std::fmt;

pub mod blocks;

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
        }
    }
}

impl fmt::Display for TransformStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "read={} inserted={} skipped={} rejected={}",
            self.read, self.inserted, self.skipped, self.rejected
        )
    }
}
