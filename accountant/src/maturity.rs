//! Block maturity tracker.
//!
//! Polls Kaspa's DAG state and drives the `block` table's
//! lifecycle from `submitted_to_node` through `confirmed_blue`
//! to `matured` (or `orphaned`, on DAG re-org).
//!
//! ## Architecture
//!
//! The tracker is structured as a single polling loop that wakes
//! every `MaturityConfig::poll_interval` and reads (a) the current
//! virtual blue score from kaspad, (b) the set of blocks the
//! accountant has previously written in
//! `submitted_to_node` / `confirmed_blue` states. For each block
//! it derives the next state transition deterministically.
//!
//! kaspad access goes through the [`KaspadClient`] trait, not
//! directly through `kaspa_grpc_client`. Two reasons:
//!
//! 1. **Testability.** The state-machine logic is the bulk of
//!    the code, but it depends on live DAG state. Stubbing
//!    kaspad behind a trait lets the test suite cover every
//!    transition path deterministically (against an in-memory
//!    fake) without standing up a real kaspad-tn10 instance.
//! 2. **Phased delivery.** This PR (Phase 3 M3b) ships the
//!    state machine + stub. The real gRPC-backed
//!    `KaspadGrpcClient` impl lands in M3c so that the kaspad
//!    integration surface (reward extraction from coinbase tx,
//!    reconnect / timeout policy, etc.) gets its own focused
//!    review.
//!
//! ## State transitions
//!
//! ```text
//!     ┌── submitted_to_node ──┐
//!     │                       │
//!     ▼                       ▼
//!  (block in DAG)         (block not in DAG yet)
//!     │                       │
//!     ├── is_blue ──┐         └── leave; retry next cycle
//!     │             │
//!     ▼             ▼
//! confirmed_blue   (red — never appeared in selected chain)
//!     │                       │
//!     ▼                       ▼
//!  (still blue?)            orphaned
//!     │
//!     ├── yes  + (vbs − blue_score ≥ maturity_depth) → matured
//!     │   (triggers AllocationEngine, hands off the reward)
//!     │
//!     ├── yes  + (insufficient depth) → leave; retry
//!     │
//!     └── no                                       → orphaned
//! ```
//!
//! The `matured` and `orphaned` states are terminal; the tracker
//! only acts on rows in `submitted_to_node` and `confirmed_blue`.
//!
//! ## Window-size policy
//!
//! Each matured block triggers a PROP allocation over a DAA
//! window ending at the block's `daa_score`. The window's
//! `daa_start` is `block.daa_score − cfg.window_daa_span`.
//! Default span is 600 DAA scores (one minute at 10 BPS
//! post-Crescendo, ten minutes pre-Crescendo). See ADR-0014.
//!
//! ## Reward extraction
//!
//! The tracker calls [`KaspadClient::get_block`] which returns
//! a `BlockInfo` carrying `coinbase_reward_sompi`. The trait
//! deliberately does NOT expose the raw coinbase transaction —
//! the only thing downstream code cares about is the pool's
//! receivable sompi. Concrete `KaspadClient` impls own the
//! parsing logic and the pool-address-recognition policy (M3c
//! work).

#![allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use katpool_db::DbError;
use katpool_db::repo::block::{self, Block, BlockStatus};
use katpool_domain::{BlockHash, DaaScore};
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::allocation::{AllocationEngine, AllocationEngineError};

/// Default polling interval between sweeps.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(15);

/// Default coinbase maturity depth in blue blocks. Matches the
/// post-Crescendo Kaspa convention of `kaspa_consensus_core::config::params::Params::coinbase_maturity = 100`.
pub const DEFAULT_MATURITY_DEPTH: u64 = 100;

/// Default DAA-window span for PROP allocation, in DAA scores.
/// 600 DAA ≈ 60 seconds at 10 BPS post-Crescendo (≈ 10 minutes
/// pre-Crescendo). See ADR-0014.
pub const DEFAULT_WINDOW_DAA_SPAN: u64 = 600;

/// Default per-cycle batch limit on blocks transitioned.
/// Bounds the tail latency of any single sweep against a
/// pathological backlog.
pub const DEFAULT_BATCH_SIZE: i64 = 200;

/// Errors surfaced by the [`KaspadClient`] trait.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KaspadError {
    /// Transport-level failure (gRPC channel down, timeout, etc.).
    #[error("kaspad transport error: {0}")]
    Transport(String),

    /// kaspad responded with a payload the client couldn't parse.
    #[error("kaspad payload malformed: {0}")]
    Malformed(String),
}

/// Snapshot of one block's DAG status, derived from kaspad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockInfo {
    /// The block hash (echoed back from the lookup).
    pub hash: BlockHash,
    /// Blue score the block appeared at. `0` if `is_blue == false`.
    pub blue_score: u64,
    /// Whether the block currently belongs to the selected
    /// (blue) chain. A `false` here for a previously-blue block
    /// signals an orphan via DAG re-org.
    pub is_blue: bool,
    /// Sum of the coinbase transaction's outputs that pay the
    /// pool's mining address(es). Computed by the `KaspadClient`
    /// implementation; opaque to the tracker.
    pub coinbase_reward_sompi: i64,
    /// Block's DAA score. Used to compute the PROP window.
    pub daa_score: u64,
}

/// Minimal kaspad surface the maturity tracker needs.
#[async_trait]
pub trait KaspadClient: Send + Sync + 'static {
    /// Current virtual chain tip blue score.
    async fn get_virtual_blue_score(&self) -> Result<u64, KaspadError>;

    /// Look up one block's DAG status. `Ok(None)` means kaspad
    /// has never seen the hash *or* the block has been pruned —
    /// the tracker treats both identically (keep retrying for
    /// `submitted_to_node`; transition `confirmed_blue` to
    /// `orphaned` after a grace period the caller decides).
    async fn get_block(&self, hash: BlockHash) -> Result<Option<BlockInfo>, KaspadError>;
}

/// Runtime knobs for the tracker.
#[derive(Debug, Clone, Copy)]
pub struct MaturityConfig {
    /// Sweep cadence. Operator can lower this for sub-minute
    /// allocation latency; defaults to 15 s as a compromise
    /// between latency and kaspad load.
    pub poll_interval: Duration,
    /// How many blue blocks deep before a block matures.
    pub maturity_depth: u64,
    /// DAA-score span of the PROP window that ends at the
    /// matured block.
    pub window_daa_span: u64,
    /// Max blocks transitioned per sweep (bounds tail latency).
    pub batch_size: i64,
}

impl Default for MaturityConfig {
    fn default() -> Self {
        Self {
            poll_interval: DEFAULT_POLL_INTERVAL,
            maturity_depth: DEFAULT_MATURITY_DEPTH,
            window_daa_span: DEFAULT_WINDOW_DAA_SPAN,
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }
}

/// Outcome counters for one sweep.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepStats {
    /// Blocks transitioned `submitted_to_node → confirmed_blue`.
    pub confirmed_blue: u64,
    /// Blocks transitioned `confirmed_blue → matured`. Each one
    /// triggered an `AllocationEngine` call.
    pub matured: u64,
    /// Blocks transitioned to `orphaned`.
    pub orphaned: u64,
    /// Blocks the tracker examined but couldn't yet act on
    /// (kaspad hasn't seen them or they're not deep enough).
    pub still_waiting: u64,
    /// Per-block errors that didn't kill the sweep.
    pub errors: u64,
}

/// The tracker.
pub struct MaturityTracker {
    db: PgPool,
    kaspad: Arc<dyn KaspadClient>,
    engine: Arc<AllocationEngine>,
    cfg: MaturityConfig,
    instance_id: String,
}

impl MaturityTracker {
    /// Construct a tracker.
    #[must_use]
    pub const fn new(
        db: PgPool,
        kaspad: Arc<dyn KaspadClient>,
        engine: Arc<AllocationEngine>,
        cfg: MaturityConfig,
        instance_id: String,
    ) -> Self {
        Self {
            db,
            kaspad,
            engine,
            cfg,
            instance_id,
        }
    }

    /// One sweep over the active block set. Public for tests; the
    /// production wiring uses [`Self::run_loop`].
    #[allow(clippy::cognitive_complexity)]
    pub async fn run_once(&self) -> Result<SweepStats, TrackerError> {
        let virtual_blue_score = self
            .kaspad
            .get_virtual_blue_score()
            .await
            .map_err(TrackerError::Kaspad)?;
        debug!(instance = %self.instance_id, virtual_blue_score, "tracker sweep start");

        let active = block::list_by_status(
            &self.db,
            &[BlockStatus::SubmittedToNode, BlockStatus::ConfirmedBlue],
            self.cfg.batch_size,
        )
        .await
        .map_err(TrackerError::Db)?;

        let mut stats = SweepStats::default();
        for blk in active {
            match self.process_block(&blk, virtual_blue_score).await {
                Ok(BlockOutcome::ConfirmedBlue) => stats.confirmed_blue += 1,
                Ok(BlockOutcome::Matured) => stats.matured += 1,
                Ok(BlockOutcome::Orphaned) => stats.orphaned += 1,
                Ok(BlockOutcome::StillWaiting) => stats.still_waiting += 1,
                Err(e) => {
                    stats.errors += 1;
                    let hash_hex = hex::encode(&blk.hash);
                    error!(
                        instance = %self.instance_id,
                        hash = %hash_hex,
                        error = %e,
                        "tracker per-block error; continuing sweep"
                    );
                }
            }
        }

        info!(
            instance = %self.instance_id,
            virtual_blue_score,
            ?stats,
            "tracker sweep done"
        );
        Ok(stats)
    }

    /// Run the sweep on a fixed interval until `shutdown`
    /// fires. Designed to be `tokio::spawn`-ed.
    pub async fn run_loop(self, mut shutdown: watch::Receiver<bool>) -> Result<(), TrackerError> {
        let mut interval = time::interval(self.cfg.poll_interval);
        // Skip the immediate tick so the first sweep happens after
        // poll_interval (lets the consumer warm up).
        interval.tick().await;
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!(instance = %self.instance_id, "tracker shutdown requested; exiting");
                        return Ok(());
                    }
                }
                _ = interval.tick() => {
                    if let Err(e) = self.run_once().await {
                        // A whole-sweep error (e.g. kaspad
                        // transport down) is logged but doesn't
                        // kill the loop — the next interval tick
                        // retries.
                        warn!(instance = %self.instance_id, error = %e, "tracker sweep failed; will retry");
                    }
                }
            }
        }
    }

    async fn process_block(
        &self,
        blk: &Block,
        virtual_blue_score: u64,
    ) -> Result<BlockOutcome, TrackerError> {
        let hash = bytes_to_hash(&blk.hash).ok_or_else(|| TrackerError::Malformed {
            reason: "block.hash is not 32 bytes",
        })?;

        match blk.status {
            BlockStatus::SubmittedToNode => self.process_submitted(hash, blk).await,
            BlockStatus::ConfirmedBlue => {
                self.process_confirmed_blue(hash, blk, virtual_blue_score)
                    .await
            }
            BlockStatus::Found | BlockStatus::Matured | BlockStatus::Orphaned => {
                Ok(BlockOutcome::StillWaiting)
            }
        }
    }

    async fn process_submitted(
        &self,
        hash: BlockHash,
        _blk: &Block,
    ) -> Result<BlockOutcome, TrackerError> {
        let Some(info) = self
            .kaspad
            .get_block(hash)
            .await
            .map_err(TrackerError::Kaspad)?
        else {
            // kaspad doesn't know the block yet — retry next sweep.
            return Ok(BlockOutcome::StillWaiting);
        };
        if !info.is_blue {
            // Block reached kaspad but isn't on the selected chain
            // yet. Could become blue with a reorg. Leave alone.
            return Ok(BlockOutcome::StillWaiting);
        }
        block::mark_confirmed_blue(&self.db, hash, info.blue_score as i64)
            .await
            .map_err(TrackerError::Db)?;
        info!(instance = %self.instance_id, hash = %hash, blue_score = info.blue_score, "block confirmed blue");
        Ok(BlockOutcome::ConfirmedBlue)
    }

    async fn process_confirmed_blue(
        &self,
        hash: BlockHash,
        blk: &Block,
        virtual_blue_score: u64,
    ) -> Result<BlockOutcome, TrackerError> {
        let Some(info) = self
            .kaspad
            .get_block(hash)
            .await
            .map_err(TrackerError::Kaspad)?
        else {
            // Previously-blue block no longer present in DAG → orphan.
            block::mark_orphaned(&self.db, hash)
                .await
                .map_err(TrackerError::Db)?;
            warn!(instance = %self.instance_id, hash = %hash, "block orphaned (no longer in DAG)");
            return Ok(BlockOutcome::Orphaned);
        };
        if !info.is_blue {
            // Reorged out of the selected chain.
            block::mark_orphaned(&self.db, hash)
                .await
                .map_err(TrackerError::Db)?;
            warn!(
                instance = %self.instance_id,
                hash = %hash,
                old_blue_score = blk.blue_score,
                "block orphaned (reorged to red)"
            );
            return Ok(BlockOutcome::Orphaned);
        }
        // Still blue — check depth.
        if virtual_blue_score < info.blue_score
            || virtual_blue_score - info.blue_score < self.cfg.maturity_depth
        {
            return Ok(BlockOutcome::StillWaiting);
        }
        // Matured. Hand off to the allocation engine.
        let daa_end = DaaScore::new(info.daa_score);
        let daa_start = DaaScore::new(info.daa_score.saturating_sub(self.cfg.window_daa_span));
        let outcome = self
            .engine
            .allocate_matured_block(hash, info.coinbase_reward_sompi, daa_start, daa_end)
            .await
            .map_err(TrackerError::Engine)?;
        info!(
            instance = %self.instance_id,
            hash = %hash,
            reward = info.coinbase_reward_sompi,
            allocation = ?outcome,
            "block matured + allocated"
        );
        Ok(BlockOutcome::Matured)
    }
}

#[derive(Debug)]
enum BlockOutcome {
    ConfirmedBlue,
    Matured,
    Orphaned,
    StillWaiting,
}

/// Top-level tracker errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TrackerError {
    /// kaspad upstream call failed.
    #[error("kaspad: {0}")]
    Kaspad(KaspadError),
    /// Database error.
    #[error("db: {0}")]
    Db(DbError),
    /// `AllocationEngine` failed mid-cycle.
    #[error("allocation engine: {0}")]
    Engine(AllocationEngineError),
    /// Schema-level invariant we couldn't recover from.
    #[error("malformed schema row: {reason}")]
    Malformed {
        /// Human-readable reason.
        reason: &'static str,
    },
}

const fn bytes_to_hash(bytes: &[u8]) -> Option<BlockHash> {
    if bytes.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(bytes);
    Some(BlockHash::from_bytes(arr))
}
