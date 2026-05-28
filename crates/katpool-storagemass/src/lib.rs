//! KIP-9 (Storage Mass) and KIP-13 (Transient Storage Mass) calculator.
//!
//! Wraps [`kaspa_consensus_core::mass::MassCalculator`] so payout code
//! never drifts from kaspad's consensus rules. See `docs/kips.md`.

#![cfg_attr(not(test), warn(missing_docs))]

mod evaluator;
mod planner;
mod tx_build;
mod types;

pub use evaluator::{
    MAINNET_MAX_BLOCK_MASS, MIN_PAYOUT_OUTPUT_SOMPI, MassEvaluationError, MassEvaluator, TxMass,
};
pub use planner::plan_batches;
pub use tx_build::build_populated;
pub use types::{PayoutRecipient, PlanBatchesResult, PlannedBatch, TreasuryUtxo};

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
