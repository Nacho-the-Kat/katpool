//! Pool accountant.
//!
//! Subscribes to the bridge's `PoolEvent` broadcast channel and converts
//! share + block events into per-miner balance deltas using deterministic
//! PROP allocation:
//!
//! - On `BlockAccepted`: `miner_reward * 9925/10000` proportionally allocated
//!   to shares with `daa_score <= block.daa_score`. Pool fee `75/10000` is
//!   retained, then split 33/67 into `nacho_rebate_kas` accrual and pool
//!   revenue.
//! - Fallback: time-weighted estimated difficulty if no shares.
//!
//! Real implementation lands in Phase 3.

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
