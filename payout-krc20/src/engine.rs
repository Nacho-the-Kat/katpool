//! The KRC-20 NACHO payout engine: a single-leader periodic loop that drives
//! one rebate cycle per DAA window through plan → settle → credit → reconcile.
//!
//! Mirrors the Phase 4 KAS engine ([`payout_kas::PayoutEngine`]) and reuses its
//! safety properties:
//!
//! - **Single leader.** Each tick is guarded by a Postgres session advisory
//!   lock ([`katpool_idempotency::AdvisoryLock`]); a non-leader instance skips
//!   the tick. The lock is leak-safe (released on connection close), so
//!   leadership always recovers — running multiple `katpool` replicas is safe.
//! - **Idempotent identity.** The cycle window comes from [`cycle_window`], so
//!   ticks inside one DAA bucket resume the same cycle
//!   ([`resume_or_plan_krc20_cycle`]); amounts/recipients never shift under an
//!   in-flight commit/reveal.
//! - **Confirmation never lags the bucket.** Construction rejects a
//!   `cycle_span_daa` that is not strictly greater than
//!   [`KAS_PAYOUT_CONFIRMATION_DAA`] (the same finality depth the executor
//!   confirms against), so a cycle's transfers always confirm before the
//!   window rolls over.
//! - **Safe-by-default.** [`ExecutionMode::DryRun`] settles without recording
//!   or broadcasting (M5.4b) and never credits; only a live tick moves funds or
//!   mutates `nacho_rebate.paid_sompi`.

use std::time::Duration;

use kaspa_addresses::Address;
use katpool_db::repo::payout::PayoutCycleStatus;
use katpool_idempotency::{AdvisoryLock, advisory_key};
use katpool_secrets::TreasurySecret;
use payout_kas::{ExecutionMode, KAS_PAYOUT_CONFIRMATION_DAA, KaspadClient, cycle_window};
use secp256k1::Keypair;
use sqlx::PgPool;
use tokio::sync::watch;
use tokio::time;
use tracing::{debug, info, warn};

use crate::cycle::{
    CreditReport, Krc20CycleError, Krc20CycleParams, credit_completed_transfers,
    reconcile_krc20_cycle_status, resume_or_plan_krc20_cycle,
};
use crate::execute::{Krc20ExecuteError, Krc20FeeConfig, SettleReport, settle_pending};
use crate::quote::FloorPriceSource;

/// Errors from the KRC-20 payout engine.
#[derive(Debug, thiserror::Error)]
pub enum Krc20EngineError {
    /// Database / advisory-lock failure.
    #[error(transparent)]
    Db(#[from] katpool_db::DbError),

    /// Cycle planning / crediting / reconciliation failure.
    #[error(transparent)]
    Cycle(#[from] Krc20CycleError),

    /// Settlement (sign / submit / confirm) failure.
    #[error(transparent)]
    Execute(#[from] Krc20ExecuteError),

    /// kaspad RPC failure.
    #[error(transparent)]
    Kaspad(#[from] payout_kas::KaspadError),

    /// Treasury key could not be parsed into a keypair.
    #[error("treasury key: {0}")]
    Key(secp256k1::Error),

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
pub struct Krc20PayoutEngineConfig {
    /// Instance label for logs/metrics.
    pub instance_id: String,
    /// How often to attempt a tick.
    pub poll_interval: Duration,
    /// DAA width of one payout cycle (cadence + idempotency bucket).
    pub cycle_span_daa: u64,
    /// Live broadcast or dry-run rehearsal.
    pub mode: ExecutionMode,
    /// Advisory-lock namespace (hashed to the leader key). Distinct from the
    /// KAS engine's namespace so the two engines never contend.
    pub lock_namespace: String,
    /// Commit/reveal fees.
    pub fees: Krc20FeeConfig,
    /// Minimum pending KAS-sompi for a wallet to be selected (coarse filter).
    pub min_pending_sompi: i64,
    /// Minimum converted NACHO base units worth a reveal (dust gate).
    pub min_nacho_base_units: u128,
    /// Token ticker to quote and inscribe.
    pub ticker: String,
    /// KAS-sompi locked into each commit P2SH output.
    pub commit_amount_sompi: u64,
    /// Cap on recipients planned and transfers settled per tick.
    pub batch_limit: i64,
}

impl Krc20PayoutEngineConfig {
    fn cycle_params(
        &self,
        daa_start: katpool_domain::DaaScore,
        daa_end: katpool_domain::DaaScore,
    ) -> Krc20CycleParams {
        Krc20CycleParams {
            daa_start,
            daa_end,
            min_pending_sompi: self.min_pending_sompi,
            min_nacho_base_units: self.min_nacho_base_units,
            ticker: self.ticker.clone(),
            commit_amount_sompi: self.commit_amount_sompi,
            limit: self.batch_limit,
        }
    }
}

/// Result of a single tick.
#[derive(Debug, Clone)]
pub enum Krc20TickOutcome {
    /// Another instance held the leader lock; this tick did no work.
    SkippedNotLeader,
    /// This instance was leader and ran a cycle.
    Ran(Box<Krc20TickReport>),
}

/// Details of a tick that ran.
#[derive(Debug, Clone)]
pub struct Krc20TickReport {
    /// Cycle that was driven.
    pub cycle_id: i64,
    /// Virtual DAA score observed at tick start.
    pub virtual_daa: u64,
    /// Window start (inclusive).
    pub daa_start: u64,
    /// Window end (exclusive).
    pub daa_end: u64,
    /// Settlement outcome.
    pub settle: SettleReport,
    /// Crediting outcome (empty in dry-run).
    pub credit: CreditReport,
    /// Cycle status after reconcile.
    pub status: PayoutCycleStatus,
}

/// The KRC-20 payout engine. Owns its kaspad client, treasury key, and
/// floor-price source for the life of the loop.
pub struct Krc20PayoutEngine<C: KaspadClient, Q: FloorPriceSource> {
    pool: PgPool,
    client: C,
    secret: TreasurySecret,
    treasury_address: Address,
    quote: Q,
    config: Krc20PayoutEngineConfig,
    lock_key: i64,
}

impl<C, Q> Krc20PayoutEngine<C, Q>
where
    C: KaspadClient + Sync,
    Q: FloorPriceSource,
{
    /// Build an engine, validating the span invariant.
    ///
    /// # Errors
    ///
    /// [`Krc20EngineError::SpanTooSmall`] if `cycle_span_daa` does not exceed
    /// the confirmation depth.
    pub fn new(
        pool: PgPool,
        client: C,
        secret: TreasurySecret,
        treasury_address: Address,
        quote: Q,
        config: Krc20PayoutEngineConfig,
    ) -> Result<Self, Krc20EngineError> {
        if config.cycle_span_daa <= KAS_PAYOUT_CONFIRMATION_DAA {
            return Err(Krc20EngineError::SpanTooSmall {
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
            quote,
            config,
            lock_key,
        })
    }

    /// Attempt one tick. Acquires the leader lock; if another instance holds
    /// it, returns [`Krc20TickOutcome::SkippedNotLeader`] without doing work.
    ///
    /// # Errors
    ///
    /// See [`Krc20EngineError`].
    pub async fn run_once(&self) -> Result<Krc20TickOutcome, Krc20EngineError> {
        let Some(lock) = AdvisoryLock::try_acquire(&self.pool, self.lock_key).await? else {
            debug!(instance = %self.config.instance_id, "krc20 payout lock held elsewhere; skipping tick");
            return Ok(Krc20TickOutcome::SkippedNotLeader);
        };

        let result = self.run_locked().await;

        // Always release, regardless of the work result.
        if let Err(e) = lock.release().await {
            warn!(instance = %self.config.instance_id, error = %e, "failed to release krc20 payout lock");
        }
        result
    }

    async fn run_locked(&self) -> Result<Krc20TickOutcome, Krc20EngineError> {
        let keypair = Keypair::from_seckey_slice(secp256k1::SECP256K1, self.secret.expose_secret())
            .map_err(Krc20EngineError::Key)?;
        let xonly = keypair.x_only_public_key().0.serialize();
        let prefix = self.treasury_address.prefix;

        let virtual_daa = self.client.virtual_daa_score().await?;
        let (daa_start, daa_end) = cycle_window(virtual_daa, self.config.cycle_span_daa);
        let params = self.config.cycle_params(daa_start, daa_end);

        // Plan or resume the cycle for this window (quotes the floor price once;
        // fails the tick closed if the quote source is degraded).
        let state =
            resume_or_plan_krc20_cycle(&self.pool, &self.quote, &xonly, prefix, &params).await?;
        let cycle_id = state.cycle.id;

        // Drive every open transfer one step (record-before-broadcast,
        // crash-safe, idempotent). Dry-run records/broadcasts nothing.
        let settle = settle_pending(
            &self.pool,
            &self.client,
            &self.secret,
            &self.treasury_address,
            self.config.fees,
            self.config.batch_limit,
            self.config.mode,
        )
        .await?;

        // Crediting mutates `nacho_rebate.paid_sompi` (real accounting), so it
        // only runs on a live tick. A dry-run reports an empty credit.
        let credit = if self.config.mode.is_dry_run() {
            CreditReport::default()
        } else {
            credit_completed_transfers(&self.pool, self.config.batch_limit).await?
        };

        let status = reconcile_krc20_cycle_status(&self.pool, cycle_id).await?;

        info!(
            instance = %self.config.instance_id,
            cycle_id,
            virtual_daa,
            daa_start = daa_start.value(),
            daa_end = daa_end.value(),
            commits = settle.commits_broadcast,
            reveals = settle.reveals_broadcast,
            rebroadcasts = settle.rebroadcasts,
            completed = settle.completed,
            settle_pending = settle.pending,
            settle_errors = settle.errors.len(),
            credited = credit.credited,
            status = ?status,
            dry_run = self.config.mode.is_dry_run(),
            "krc20 payout tick complete"
        );

        Ok(Krc20TickOutcome::Ran(Box::new(Krc20TickReport {
            cycle_id,
            virtual_daa,
            daa_start: daa_start.value(),
            daa_end: daa_end.value(),
            settle,
            credit,
            status,
        })))
    }

    /// Run the periodic loop until `shutdown` flips to `true`.
    ///
    /// # Errors
    ///
    /// Only construction/invariant errors propagate; per-tick failures are
    /// logged and retried on the next interval.
    pub async fn run_loop(
        self,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<(), Krc20EngineError> {
        let mut interval = time::interval(self.config.poll_interval);
        // Skip the immediate first tick so startup does not double-fire.
        interval.tick().await;
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!(instance = %self.config.instance_id, "krc20 payout engine shutdown requested; exiting");
                        return Ok(());
                    }
                }
                _ = interval.tick() => {
                    if let Err(e) = self.run_once().await {
                        warn!(instance = %self.config.instance_id, error = %e, "krc20 payout tick failed; will retry");
                    }
                }
            }
        }
    }
}
