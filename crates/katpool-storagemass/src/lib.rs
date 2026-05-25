//! KIP-9 (Storage Mass) and KIP-13 (Transient Storage Mass) calculator.
//!
//! Pure functions only — no I/O, no async, no global state. Every result is
//! a deterministic function of the inputs. This is the crate that protects
//! us from repeating the May 1 NACHO payout failure: every outbound
//! transaction is run through the batcher here before signing.
//!
//! The full formula and constants are documented in `docs/kips.md` and
//! land alongside the implementation in Phase 4.
//!
//! References:
//! - KIP-9: <https://github.com/kaspanet/kips/blob/master/kip-0009.md>
//! - KIP-13: <https://github.com/kaspanet/kips/blob/master/kip-0013.md>
//! - rusty-kaspa reference impl: `consensus/core/src/mass.rs`

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
