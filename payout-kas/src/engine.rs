//! The KAS payout engine: a single-leader periodic loop that drives one cycle
//! per DAA window through plan → broadcast → confirm → reconcile.
//!
//! ## Safety properties
//!
//! - **Single leader.** Each tick is guarded by a Postgres session advisory
//!   lock ([`katpool_idempotency::AdvisoryLock`]); a non-leader instance skips
//!   the tick. The lock is leak-safe (released on connection close even on a
//!   panic), so leadership always recovers.
//! - **Idempotent identity.** The cycle window comes from [`cycle_window`], so
//!   ticks inside one DAA bucket resume the same cycle; the M4.4 state machine
//!   guarantees a resumed cycle never re-pays a recipient.
//! - **Confirmation never lags the bucket.** The engine confirms the *current*
//!   cycle each tick. Constructing the engine rejects a `cycle_span_daa` that
//!   is not strictly greater than [`KAS_PAYOUT_CONFIRMATION_DAA`], so a cycle's
//!   payouts always confirm well before the window rolls over.
//! - **Dry-run.** [`ExecutionMode::DryRun`] signs and verifies but neither
//!   records nor broadcasts — the rehearsal path (M4.8).

use std::time::Duration;

use kaspa_addresses::Address;
use katpool_db::repo::payout::{self, PayoutCycleStatus};
use katpool_idempotency::{AdvisoryLock, advisory_key};
use katpool_secrets::TreasurySecret;
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::time;
use tracing::{debug, info, warn};

use crate::client::KaspadClient;
use crate::confirm::KAS_PAYOUT_CONFIRMATION_DAA;
use crate::cycle::{CycleState, reconcile_cycle_status, resume_or_plan_kas_cycle};
use crate::execute::{
    ConfirmReport, ExecuteError, ExecutionMode, ExecutionReport, broadcast_cycle, confirm_cycle,
};
use crate::plan::PlanKasCycleParams;
use crate::window::cycle_window;

/// Errors from the payout engine.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// Database / advisory-lock failure.
    #[error(transparent)]
    Db(#[from] katpool_db::DbError),

    /// Cycle planning / reconciliation failure.
    #[error(transparent)]
    Payout(#[from] crate::error::PayoutKasError),

    /// Sign / submit / confirm failure.
    #[error(transparent)]
    Execute(#[from] ExecuteError),

    /// kaspad RPC failure.
    #[error(transparent)]
    Kaspad(#[from] crate::client::KaspadError),

    /// `cycle_span_daa` is too small to guarantee in-window confirmation.
    #[error("cycle_span_daa ({span}) must exceed confirmation depth ({depth})")]
    SpanTooSmall {
        /// Configured span.
        span: u64,
        /// Required confirmation depth.
        depth: u64,
    },
}

/// Engine configuration. Built from runtime config / env in the binary.
#[derive(Debug, Clone)]
pub struct PayoutEngineConfig {
    /// Instance label for logs/metrics.
    pub instance_id: String,
    /// How often to attempt a tick.
    pub poll_interval: Duration,
    /// DAA width of one payout cycle (cadence + idempotency bucket).
    pub cycle_span_daa: u64,
    /// Minimum payable balance (sompi) to include a wallet.
    pub threshold_sompi: i64,
    /// Live broadcast or dry-run rehearsal.
    pub mode: ExecutionMode,
    /// Advisory-lock namespace (hashed to the leader key).
    pub lock_namespace: String,
}

/// Result of a single tick.
#[derive(Debug, Clone)]
pub enum TickOutcome {
    /// Another instance held the leader lock; this tick did no work.
    SkippedNotLeader,
    /// This instance was leader and ran a cycle.
    Ran(Box<TickReport>),
}

/// Details of a tick that ran.
#[derive(Debug, Clone)]
pub struct TickReport {
    /// Cycle that was driven.
    pub cycle_id: i64,
    /// Virtual DAA score observed at tick start.
    pub virtual_daa: u64,
    /// Window start (inclusive).
    pub daa_start: u64,
    /// Window end (exclusive).
    pub daa_end: u64,
    /// Broadcast outcome.
    pub broadcast: ExecutionReport,
    /// Confirmation outcome.
    pub confirm: ConfirmReport,
    /// Cycle status after reconcile.
    pub status: PayoutCycleStatus,
}

/// The payout engine. Owns its kaspad client and treasury key for the life of
/// the loop.
pub struct PayoutEngine<C: KaspadClient> {
    pool: PgPool,
    client: C,
    secret: TreasurySecret,
    treasury_address: Address,
    config: PayoutEngineConfig,
    lock_key: i64,
}

impl<C: KaspadClient> PayoutEngine<C> {
    /// Build an engine, validating the span invariant.
    pub fn new(
        pool: PgPool,
        client: C,
        secret: TreasurySecret,
        treasury_address: Address,
        config: PayoutEngineConfig,
    ) -> Result<Self, EngineError> {
        if config.cycle_span_daa <= KAS_PAYOUT_CONFIRMATION_DAA {
            return Err(EngineError::SpanTooSmall {
                span: config.cycle_span_daa,
                depth: KAS_PAYOUT_CONFIRMATION_DAA,
            });
        }
        let lock_key = advisory_key(&config.lock_namespace);
        Ok(Self {
            pool,
            client,
            secret,
            treasury_address,
            config,
            lock_key,
        })
    }

    /// Attempt one tick. Acquires the leader lock; if another instance holds
    /// it, returns [`TickOutcome::SkippedNotLeader`] without doing work.
    pub async fn run_once(&self) -> Result<TickOutcome, EngineError> {
        let Some(lock) = AdvisoryLock::try_acquire(&self.pool, self.lock_key).await? else {
            debug!(instance = %self.config.instance_id, "payout lock held elsewhere; skipping tick");
            return Ok(TickOutcome::SkippedNotLeader);
        };

        let result = self.run_locked().await;

        // Always release, regardless of the work result.
        if let Err(e) = lock.release().await {
            warn!(instance = %self.config.instance_id, error = %e, "failed to release payout lock");
        }
        result
    }

    async fn run_locked(&self) -> Result<TickOutcome, EngineError> {
        let virtual_daa = self.client.virtual_daa_score().await?;
        let (daa_start, daa_end) = cycle_window(virtual_daa, self.config.cycle_span_daa);
        let params = PlanKasCycleParams {
            daa_start,
            daa_end,
            threshold_sompi: self.config.threshold_sompi,
        };

        let state = resume_or_plan_kas_cycle(&self.pool, params).await?;
        let cycle_id = state.cycle.id;

        let broadcast = broadcast_cycle(
            &self.pool,
            &self.client,
            &self.secret,
            &self.treasury_address,
            &state,
            self.config.mode,
        )
        .await?;

        // Reload statuses (broadcast just advanced rows) without re-auditing a
        // resume, then confirm + reconcile.
        let reloaded = CycleState {
            cycle: payout::get_cycle(&self.pool, cycle_id).await?,
            payouts: payout::list_for_cycle(&self.pool, cycle_id).await?,
        };
        let confirm =
            confirm_cycle(&self.pool, &self.client, &self.treasury_address, &reloaded).await?;
        let status = reconcile_cycle_status(&self.pool, cycle_id).await?;

        info!(
            instance = %self.config.instance_id,
            cycle_id,
            virtual_daa,
            daa_start = daa_start.value(),
            daa_end = daa_end.value(),
            batches = broadcast.planned_batches,
            submitted = broadcast.submitted_payouts,
            accepted = confirm.accepted,
            confirmed = confirm.confirmed,
            status = ?status,
            dry_run = self.config.mode.is_dry_run(),
            "payout tick complete"
        );

        Ok(TickOutcome::Ran(Box::new(TickReport {
            cycle_id,
            virtual_daa,
            daa_start: daa_start.value(),
            daa_end: daa_end.value(),
            broadcast,
            confirm,
            status,
        })))
    }

    /// Run the periodic loop until `shutdown` flips to `true`.
    pub async fn run_loop(self, mut shutdown: watch::Receiver<bool>) -> Result<(), EngineError> {
        let mut interval = time::interval(self.config.poll_interval);
        // Skip the immediate first tick so startup does not double-fire.
        interval.tick().await;
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!(instance = %self.config.instance_id, "payout engine shutdown requested; exiting");
                        return Ok(());
                    }
                }
                _ = interval.tick() => {
                    if let Err(e) = self.run_once().await {
                        warn!(instance = %self.config.instance_id, error = %e, "payout tick failed; will retry");
                    }
                }
            }
        }
    }
}
