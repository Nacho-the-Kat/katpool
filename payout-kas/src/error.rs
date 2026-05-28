//! Typed errors for the KAS payout engine.

use katpool_db::DbError;

/// Errors from [`crate::plan_kas_cycle`].
#[derive(Debug, thiserror::Error)]
pub enum PlanKasCycleError {
    /// Database failure.
    #[error(transparent)]
    Db(#[from] DbError),
}
