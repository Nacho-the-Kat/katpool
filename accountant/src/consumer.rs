//! Event consumer.
//!
//! Drains a `tokio::sync::broadcast::Receiver<PoolEvent>` and
//! mirrors every event into the new schema's `wallet`, `worker`,
//! `share`, and `block` tables.
//!
//! ## Lifecycle
//!
//! ```text
//!     ┌── ShareCredited   → wallet::ensure  → worker::ensure → share::insert_credited
//!     │
//!     ├── ShareRejected   → metric tick only (per `docs/decisions/0012`,
//!     │                     reject persistence is M2 scope; M1 keeps the
//!     │                     hot path lean).
//!     │
//!     ├── BlockFound      → wallet::ensure  → worker::ensure → block::ensure
//!     │                                                       (status='found')
//!     │
//!     └── BlockAccepted   → block::mark_submitted (no-op if no prior found)
//! ```
//!
//! ## Lag tolerance
//!
//! The broadcast channel is bounded. A slow consumer eventually
//! sees `RecvError::Lagged(n)` — the consumer increments the
//! `ks_accountant_events_lagged_total` counter and continues
//! draining. We do **not** terminate the consumer on lag: missed
//! shares are unrecoverable (the bridge channel isn't durable),
//! but the consumer must remain healthy so subsequent events
//! still land.
//!
//! ## Channel close
//!
//! `RecvError::Closed` ends the consumer task with `Ok(())`.
//! Callers can `await` the returned `JoinHandle` to observe a
//! clean shutdown.
//!
//! ## Failure isolation
//!
//! Per-event DB errors are logged, counted, and swallowed. A
//! single bad event must never poison the consumer — Phase 1's
//! `PoolEvent` types validate everything domain-side, so a DB
//! constraint failure is almost always either a transient
//! Postgres issue (resolved on the next event) or a bug we want
//! the metric tick to surface.

use katpool_db::repo::block::{self, EnsureOutcome};
use katpool_db::repo::share_reject::{self, DbShareRejectReason};
use katpool_db::repo::{share, wallet, worker};
use katpool_domain::PoolEvent;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::error::EventError;
use crate::metrics::{
    record_block_transition, record_event, record_event_error, record_lag, record_share_insert,
};

/// The five network identifiers the schema's `wallet_address_format`
/// CHECK constraint accepts (see
/// `crates/katpool-db/migrations/20260526000000_bootstrap.sql`).
///
/// Keep this list in lock-step with the migration: a value the
/// schema rejects would cause every `wallet::ensure` to fail with
/// SQLSTATE `23514`, which is exactly the M3d `wallet_ensure`
/// production incident this constant was introduced to prevent.
pub const VALID_NETWORKS: &[&str] = &["mainnet", "testnet-10", "testnet-11", "devnet", "simnet"];

/// Configuration carried by every consumer instance. Cheap to
/// clone (it's all `Arc`-able internals + a small instance label).
#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    /// Stable label used by every metric the consumer emits.
    /// Typical value: the systemd instance name (e.g. `primary`,
    /// `shadow`).
    pub instance_id: String,
    /// Kaspa network identifier — passed verbatim to
    /// [`katpool_db::repo::wallet::ensure`]. Must be one of
    /// [`VALID_NETWORKS`]; otherwise every wallet upsert would fail
    /// the schema's `wallet_address_format` CHECK constraint at
    /// runtime. Validated at construction time so misconfiguration
    /// fails fast instead of silently dropping every share /
    /// block event.
    pub network: String,
}

impl ConsumerConfig {
    /// Construct with the given instance id and network.
    ///
    /// # Errors
    /// Returns an error if `network` is not one of [`VALID_NETWORKS`].
    pub fn new(instance_id: String, network: String) -> Result<Self, ConsumerConfigError> {
        if !VALID_NETWORKS.contains(&network.as_str()) {
            return Err(ConsumerConfigError::InvalidNetwork {
                supplied: network,
                allowed: VALID_NETWORKS,
            });
        }
        Ok(Self {
            instance_id,
            network,
        })
    }
}

/// Errors raised by [`ConsumerConfig::new`].
#[derive(Debug, thiserror::Error)]
pub enum ConsumerConfigError {
    /// The supplied `network` is not in [`VALID_NETWORKS`]; the DB's
    /// `wallet_address_format` CHECK constraint would reject every
    /// wallet upsert.
    #[error("invalid network `{supplied}` (allowed: {allowed:?})")]
    InvalidNetwork {
        /// The rejected value the caller passed.
        supplied: String,
        /// The set of values the schema accepts.
        allowed: &'static [&'static str],
    },
}

/// The consumer task. Holds the DB pool + the consumer's
/// configuration; `run` consumes both and drives the event loop.
pub struct EventConsumer {
    db: PgPool,
    cfg: ConsumerConfig,
}

impl EventConsumer {
    /// Construct a consumer ready to be `run`.
    #[must_use]
    pub const fn new(db: PgPool, cfg: ConsumerConfig) -> Self {
        Self { db, cfg }
    }

    /// Drive the consumer until the broadcast channel closes.
    ///
    /// Returns when the broadcast channel is closed by every
    /// sender. Per-event errors are logged + counted, never
    /// returned.
    pub async fn run(self, mut rx: broadcast::Receiver<PoolEvent>) -> Result<(), anyhow::Error> {
        info!(instance = %self.cfg.instance_id, "accountant consumer starting");
        loop {
            match rx.recv().await {
                Ok(event) => self.handle_event(event).await,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    record_lag(&self.cfg.instance_id);
                    warn!(
                        instance = %self.cfg.instance_id,
                        skipped = n,
                        "broadcast channel lag; events dropped"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!(instance = %self.cfg.instance_id, "broadcast channel closed; consumer exiting");
                    return Ok(());
                }
            }
        }
    }

    /// Single-event dispatch. Public for testing — callers in
    /// production should always go through `run`.
    pub async fn handle_event(&self, event: PoolEvent) {
        match event {
            PoolEvent::ShareCredited {
                wallet,
                worker,
                difficulty,
                daa_score,
                ts: _,
                correlation_id,
            } => {
                let variant = "share_credited";
                record_event(&self.cfg.instance_id, variant);
                if let Err(e) = self
                    .handle_share_credited(&wallet, &worker, difficulty, daa_score, correlation_id)
                    .await
                {
                    self.log_event_error(variant, &e, &correlation_id);
                }
            }
            PoolEvent::ShareRejected {
                wallet,
                worker,
                reason,
                ts: _,
                correlation_id,
            } => {
                let variant = "share_rejected";
                record_event(&self.cfg.instance_id, variant);
                if let Err(e) = self
                    .handle_share_rejected(&wallet, &worker, reason, correlation_id)
                    .await
                {
                    self.log_event_error(variant, &e, &correlation_id);
                }
            }
            PoolEvent::BlockFound {
                wallet,
                worker,
                hash,
                daa_score,
                ts: _,
                correlation_id,
            } => {
                let variant = "block_found";
                record_event(&self.cfg.instance_id, variant);
                if let Err(e) = self
                    .handle_block_found(&wallet, &worker, hash, daa_score, correlation_id)
                    .await
                {
                    self.log_event_error(variant, &e, &correlation_id);
                }
            }
            PoolEvent::BlockAccepted {
                hash,
                ts: _,
                correlation_id,
            } => {
                let variant = "block_accepted";
                record_event(&self.cfg.instance_id, variant);
                if let Err(e) = self.handle_block_accepted(hash, correlation_id).await {
                    self.log_event_error(variant, &e, &correlation_id);
                }
            }
            // `PoolEvent` is `#[non_exhaustive]` by design; we
            // must keep this arm so adding a new variant upstream
            // doesn't break the build, but log loudly so an
            // operator knows the bridge added something the
            // accountant doesn't yet understand.
            other => {
                record_event(&self.cfg.instance_id, "unknown");
                warn!(event = ?other, "accountant received unknown PoolEvent variant");
            }
        }
    }

    async fn handle_share_rejected(
        &self,
        wallet_addr: &katpool_domain::WalletAddress,
        worker_name: &katpool_domain::WorkerName,
        reason: katpool_domain::ShareRejectReason,
        correlation_id: katpool_domain::CorrelationId,
    ) -> Result<(), EventError> {
        // Translate first so we can bail before opening a tx if
        // the reason has no schema mapping (recoverable, surfaces
        // as a metric tick via the caller's error-logging path).
        let db_reason = DbShareRejectReason::try_from(reason)
            .map_err(|e| EventError::UnknownRejectReason { reason: e.reason })?;

        let mut tx = self
            .db
            .begin()
            .await
            .map_err(katpool_db::DbError::from)
            .map_err(EventError::ShareRejectInsert)?;
        let w = wallet::ensure(&mut *tx, wallet_addr, &self.cfg.network)
            .await
            .map_err(EventError::WalletEnsure)?;
        let wk = worker::ensure(&mut *tx, w.id, worker_name)
            .await
            .map_err(EventError::WorkerEnsure)?;
        share_reject::insert(&mut *tx, w.id, wk.id, db_reason, correlation_id)
            .await
            .map_err(EventError::ShareRejectInsert)?;
        tx.commit()
            .await
            .map_err(katpool_db::DbError::from)
            .map_err(EventError::ShareRejectInsert)?;
        Ok(())
    }

    async fn handle_share_credited(
        &self,
        wallet_addr: &katpool_domain::WalletAddress,
        worker_name: &katpool_domain::WorkerName,
        difficulty: katpool_domain::ShareDifficulty,
        daa_score: katpool_domain::DaaScore,
        correlation_id: katpool_domain::CorrelationId,
    ) -> Result<(), EventError> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(katpool_db::DbError::from)
            .map_err(EventError::ShareInsert)?;
        let w = wallet::ensure(&mut *tx, wallet_addr, &self.cfg.network)
            .await
            .map_err(EventError::WalletEnsure)?;
        let wk = worker::ensure(&mut *tx, w.id, worker_name)
            .await
            .map_err(EventError::WorkerEnsure)?;
        share::insert_credited(
            &mut *tx,
            w.id,
            wk.id,
            None,
            difficulty,
            daa_score,
            correlation_id,
        )
        .await
        .map_err(EventError::ShareInsert)?;
        tx.commit()
            .await
            .map_err(katpool_db::DbError::from)
            .map_err(EventError::ShareInsert)?;
        record_share_insert(&self.cfg.instance_id);
        Ok(())
    }

    async fn handle_block_found(
        &self,
        wallet_addr: &katpool_domain::WalletAddress,
        worker_name: &katpool_domain::WorkerName,
        hash: katpool_domain::BlockHash,
        daa_score: katpool_domain::DaaScore,
        correlation_id: katpool_domain::CorrelationId,
    ) -> Result<(), EventError> {
        let mut tx = self
            .db
            .begin()
            .await
            .map_err(katpool_db::DbError::from)
            .map_err(EventError::BlockEnsure)?;
        let w = wallet::ensure(&mut *tx, wallet_addr, &self.cfg.network)
            .await
            .map_err(EventError::WalletEnsure)?;
        let wk = worker::ensure(&mut *tx, w.id, worker_name)
            .await
            .map_err(EventError::WorkerEnsure)?;
        // PoolEvent::BlockFound doesn't carry a nonce — that
        // belongs to the candidate template the bridge submits,
        // not to the share-validation event. Phase 1's event type
        // omits it deliberately. We record 0 here; the schema's
        // `nonce` column is informational only (no CHECK), and the
        // M3 maturity path overwrites it from the kaspad header
        // when it lands.
        let nonce: u64 = 0;
        let (_, outcome) = block::ensure(
            &mut *tx,
            hash,
            w.id,
            wk.id,
            daa_score,
            nonce,
            correlation_id,
        )
        .await
        .map_err(EventError::BlockEnsure)?;
        tx.commit()
            .await
            .map_err(katpool_db::DbError::from)
            .map_err(EventError::BlockEnsure)?;
        match outcome {
            EnsureOutcome::Inserted => record_block_transition(&self.cfg.instance_id, "found"),
            EnsureOutcome::AlreadyExisted => {
                record_block_transition(&self.cfg.instance_id, "dup_found");
                debug!(hash = %hash, "duplicate BlockFound event; ignoring");
            }
        }
        Ok(())
    }

    async fn handle_block_accepted(
        &self,
        hash: katpool_domain::BlockHash,
        correlation_id: katpool_domain::CorrelationId,
    ) -> Result<(), EventError> {
        // The repo's `mark_submitted` is itself idempotent
        // (it gates on status IN ('found', 'submitted_to_node'))
        // so we don't need our own pre-check.
        //
        // But we DO want to know whether the row existed — if it
        // didn't, the BlockAccepted arrived without a prior
        // BlockFound (race condition during consumer startup),
        // and we surface that as a metric + warning.
        let existing = block::find_by_hash(&self.db, hash)
            .await
            .map_err(EventError::BlockMarkSubmitted)?;
        if existing.is_none() {
            record_event_error(&self.cfg.instance_id, "block_accepted", "orphan");
            warn!(
                correlation_id = %correlation_id,
                hash = %hash,
                "BlockAccepted arrived without prior BlockFound; ignoring"
            );
            return Err(EventError::OrphanBlockAccepted {
                hash: hash.to_string(),
            });
        }
        block::mark_submitted(&self.db, hash)
            .await
            .map_err(EventError::BlockMarkSubmitted)?;
        record_block_transition(&self.cfg.instance_id, "submitted");
        Ok(())
    }

    fn log_event_error(
        &self,
        variant: &'static str,
        err: &EventError,
        correlation_id: &katpool_domain::CorrelationId,
    ) {
        let kind = match err {
            EventError::WalletEnsure(_) => "wallet_ensure",
            EventError::WorkerEnsure(_) => "worker_ensure",
            EventError::ShareInsert(_) => "share_insert",
            EventError::ShareRejectInsert(_) => "share_reject_insert",
            EventError::UnknownRejectReason { .. } => "unknown_reject_reason",
            EventError::BlockEnsure(_) => "block_ensure",
            EventError::BlockMarkSubmitted(_) => "block_mark_submitted",
            EventError::OrphanBlockAccepted { .. } => "orphan_block_accepted",
        };
        record_event_error(&self.cfg.instance_id, variant, kind);
        error!(
            correlation_id = %correlation_id,
            variant,
            kind,
            error = %err,
            "accountant event handler failed"
        );
    }
}

#[cfg(test)]
mod consumer_config_tests {
    //! Regression guards for the Goldshell M3d live-exercise
    //! production incident in which `ConsumerConfig` hard-coded
    //! `NETWORK = "mainnet"`, causing every `wallet::ensure` on a
    //! testnet run to fail the schema's `wallet_address_format`
    //! CHECK constraint (9,775 occurrences in a single 2.5-minute
    //! window). These tests pin both the validation behaviour and
    //! the set of accepted networks.
    use super::{ConsumerConfig, ConsumerConfigError, VALID_NETWORKS};

    #[test]
    fn accepts_every_network_the_schema_accepts() {
        for net in VALID_NETWORKS {
            let cfg = ConsumerConfig::new("test".to_owned(), (*net).to_owned())
                .unwrap_or_else(|e| panic!("{net} should be accepted: {e}"));
            assert_eq!(cfg.network, *net);
        }
    }

    #[test]
    fn rejects_obvious_typos() {
        // `kaspatest` is the address bech32 prefix, not the
        // schema-network value — easy operator confusion. Make
        // sure the constructor catches it.
        let err = ConsumerConfig::new("test".to_owned(), "kaspatest".to_owned())
            .expect_err("must reject bech32 prefix");
        assert!(matches!(err, ConsumerConfigError::InvalidNetwork { .. }));
    }

    #[test]
    fn rejects_legacy_capitalised_value() {
        // The schema CHECK is case-sensitive; `Mainnet` would
        // pass `ConsumerConfig::new` only to fail every DB call.
        // Fail fast at startup instead.
        assert!(ConsumerConfig::new("test".to_owned(), "Mainnet".to_owned()).is_err());
    }

    #[test]
    fn rejects_empty_string() {
        assert!(ConsumerConfig::new("test".to_owned(), String::new()).is_err());
    }

    #[test]
    fn schema_network_list_matches_migration() {
        // The full list is duplicated in
        // `crates/katpool-db/migrations/20260526000000_bootstrap.sql`
        // (`wallet_network_valid` CHECK). Treat this assertion as
        // a compile-time-style contract: if the migration ever
        // adds or removes a network, this test must be updated in
        // the same commit so the two stay in lock-step.
        assert_eq!(
            VALID_NETWORKS,
            &["mainnet", "testnet-10", "testnet-11", "devnet", "simnet"]
        );
    }
}
