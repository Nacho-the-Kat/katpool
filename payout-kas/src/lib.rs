//! KAS payout engine.
//!
//! Daily cron picks up miners with `balance >= thresholdAmount`, plans a
//! mass-valid set of transactions via [`katpool_storagemass`], signs them
//! using the sops-encrypted treasury key via [`katpool_secrets`], and
//! submits them through the embedded kaspad. Idempotency rests on natural
//! database keys — `payout_cycle.idempotency_key` and the per-recipient
//! `payout UNIQUE (cycle_id, wallet_id)` — which are written BEFORE any
//! transaction is signed, so a mid-cycle restart can never double-pay
//! (see [`resume_or_plan_kas_cycle`]).
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
//! ## Phase 4 milestones
//!
//! - **M4.3** — [`plan_kas_cycle`] eligibility + DB planning (this crate).
//! - **M4.4** — restart-safe cycle state machine + idempotency
//!   ([`resume_or_plan_kas_cycle`], [`reconcile_cycle_status`]).
//! - **M4.5+** — secrets, sign/submit, runtime wiring.

#![cfg_attr(not(test), warn(missing_docs))]

mod cycle;
mod error;
mod plan;

pub use cycle::{
    CycleState, PayoutStatusCounts, derive_cycle_status, reconcile_cycle_status,
    resume_or_plan_kas_cycle,
};
pub use error::PayoutKasError;
pub use katpool_db::repo::payout::DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI;
pub use plan::{PlanKasCycleParams, PlanKasCycleResult, plan_kas_cycle};

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
