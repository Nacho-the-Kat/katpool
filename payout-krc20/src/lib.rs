//! Native Rust KRC-20 NACHO rebate engine.
//!
//! Implements the kasplex envelope (`<pubkey> OP_CHECKSIG_ECDSA OP_FALSE
//! OP_IF "kasplex" OP_1 OP_0 OP_0 <json> OP_ENDIF`) via rusty-kaspa's
//! `ScriptBuilder` and runs the commit/reveal flow against the embedded
//! kaspad. Floor-price quotes are fetched via direct HTTPS to
//! `api.kaspa.com` with a circuit breaker; no Puppeteer, no headless
//! browser, no Bun.
//!
//! Real implementation lands in Phase 5.

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
