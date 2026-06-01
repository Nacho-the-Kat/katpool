//! Native Rust KRC-20 NACHO rebate engine.
//!
//! Implements the kasplex inscription envelope
//! (`<x-only pubkey> OP_CHECKSIG OP_FALSE OP_IF "kasplex" OP_0 <json>
//! OP_ENDIF`) via rusty-kaspa's [`kaspa_txscript::script_builder::ScriptBuilder`]
//! and runs the commit/reveal flow against the embedded kaspad.
//! Floor-price quotes are fetched via direct HTTPS to `api.kaspa.com` with
//! a circuit breaker; no Puppeteer, no headless browser, no Bun.
//!
//! # Status
//!
//! - **M5.1**: pure, deterministic inscription primitives
//!   ([`build_transfer_inscription`], [`commit_address`],
//!   [`reveal_signature_script`]) — byte-for-byte compatible with the live
//!   production transfer and the kasplex indexer (see ADR-0015).
//! - **M5.2**: KAS→NACHO payout conversion ([`nacho_base_units`]
//!   over an exact fixed-point [`FloorPrice`], no payout-time multiplier —
//!   the tier rebate is already in `accrued_sompi`) and the floor-price
//!   quote source ([`FloorPriceSource`] + [`KaspaComFloorPrice`]) guarded by
//!   a fail-closed [`CircuitBreaker`] (see ADR-0016).
//! - **M5.3 (this milestone)**: the mass-aware commit/reveal planner
//!   ([`plan_commit_reveal`]) — sizes signature scripts to their signed
//!   length so the reveal's `transient_storage_mass` (driven by the
//!   redeem-script push) is evaluated accurately against `max_block_mass`.
//! - Later milestones add the restart-safe payout engine that reuses the
//!   Phase 4 cycle/lock/engine scaffolding.

#![cfg_attr(not(test), warn(missing_docs))]

pub mod inscription;
pub mod plan;
pub mod quote;
pub mod rebate;

pub use inscription::{
    InscriptionError, KASPLEX_TAG, KRC20_PROTOCOL, Krc20Transfer, build_transfer_inscription,
    commit_address, commit_script_public_key, reveal_signature_script,
};
pub use plan::{
    CommitRevealConfig, DEFAULT_COMMIT_AMOUNT_SOMPI, DEFAULT_FEE_SOMPI, PlanError,
    PlannedCommitReveal, STANDARD_SIGNATURE_SCRIPT_LEN, plan_commit_reveal,
};
pub use quote::{
    BreakeredSource, CircuitBreaker, CircuitState, FloorPriceSource, KaspaComFloorPrice,
    QuoteError, parse_floor_price_response,
};
pub use rebate::{
    DEFAULT_MIN_NACHO_BASE_UNITS, DEFAULT_MIN_PENDING_SOMPI, FloorPrice, RebateError, is_payable,
    nacho_base_units,
};

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
