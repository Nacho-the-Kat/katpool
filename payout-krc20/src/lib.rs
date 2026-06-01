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
//! - **M5.1 (this milestone)**: pure, deterministic inscription primitives
//!   ([`build_transfer_inscription`], [`commit_address`],
//!   [`reveal_signature_script`]) — byte-for-byte compatible with the live
//!   production transfer and the kasplex indexer (see ADR-0014).
//! - Later milestones add eligibility/floor-price, the mass-aware
//!   commit/reveal planner, and the restart-safe payout engine that reuses
//!   the Phase 4 cycle/lock/engine scaffolding.

#![cfg_attr(not(test), warn(missing_docs))]

pub mod inscription;

pub use inscription::{
    InscriptionError, KASPLEX_TAG, KRC20_PROTOCOL, Krc20Transfer, build_transfer_inscription,
    commit_address, commit_script_public_key, reveal_signature_script,
};

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
