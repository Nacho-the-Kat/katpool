//! Restart-safe executor for the KRC-20 commit/reveal state machine.
//!
//! Drives one [`Krc20PendingTransfer`] across its lifecycle —
//! `pending → commit_submitted → reveal_submitted → completed` — reusing the
//! Phase 4 KAS scaffolding for everything chain-facing: the [`KaspadClient`]
//! RPC trait, the maturity gate ([`is_spendable`]), and the confirmation
//! policy ([`classify_confirmation`], same `KAS_PAYOUT_CONFIRMATION_DAA`
//! finality depth).
//!
//! # Crash-safety contract
//!
//! Every broadcast is preceded by an atomic *record-before-broadcast* step:
//! the deterministic txid (signature scripts excluded, see [`crate::sign`]) is
//! written to the parent payout row **and** the transfer advanced one state,
//! in a single Postgres transaction, *before* the transaction hits the wire.
//! A crash anywhere after the record re-derives the identical txid on resume
//! from the same inputs, so re-broadcast is a no-op for kaspad and never
//! double-pays.
//!
//! The resume path is defensive about UTXO drift: if a recorded commit is
//! neither on chain (its P2SH output) nor reproducible from the *current*
//! treasury UTXO set, the executor refuses to broadcast a divergent commit and
//! surfaces [`Krc20ExecuteError::CommitDrift`] for an operator instead of
//! risking a second, distinct spend.
//!
//! Scope (M5.4b): this module owns the per-transfer state machine and its
//! chain interaction. Wiring `payout.status`/cycle reconciliation and the
//! end-to-end engine is M5.5; here the `krc20_pending_transfer` row is the
//! source of truth and only the commit/reveal hashes are written onto the
//! payout row.

use kaspa_addresses::{Address, Prefix};
use kaspa_consensus_core::tx::{ScriptPublicKey, TransactionId, TransactionOutpoint};
use kaspa_txscript::pay_to_address_script;
use katpool_db::DbError;
use katpool_db::repo::payout::{self, Krc20PendingTransfer, Krc20TransferStatus, Payout};
use katpool_db::repo::wallet;
use katpool_domain::BlockHash;
use katpool_secrets::TreasurySecret;
use katpool_storagemass::{MassEvaluator, TreasuryUtxo};
use payout_kas::{
    ConfirmationInputs, ConfirmationState, ExecutionMode, KaspadClient, KaspadError,
    TreasuryUtxoSnapshot, classify_confirmation, is_spendable,
};
use secp256k1::Keypair;
use sqlx::PgPool;

use crate::inscription::{Krc20Transfer, commit_address};
use crate::plan::{CommitRevealConfig, PlanError, PlannedCommitReveal, plan_commit_reveal};
use crate::sign::{
    COMMIT_P2SH_OUTPUT_INDEX, SignError, commit_txid, reveal_txid, sign_commit, sign_reveal,
};

/// KRC-20 token ticker paid by the NACHO rebate engine.
const NACHO_TICK: &str = "NACHO";

/// Per-cycle transaction fees for the commit and reveal.
///
/// Amounts are taken per-transfer from the planned `sompi_to_miner` (the
/// commit P2SH lock); only the fees are configured here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Krc20FeeConfig {
    /// Fee paid by the commit transaction.
    pub commit_fee_sompi: u64,
    /// Fee paid by the reveal transaction.
    pub reveal_fee_sompi: u64,
}

impl Default for Krc20FeeConfig {
    fn default() -> Self {
        Self {
            commit_fee_sompi: crate::plan::DEFAULT_FEE_SOMPI,
            reveal_fee_sompi: crate::plan::DEFAULT_FEE_SOMPI,
        }
    }
}

/// The state transition a single [`advance_transfer`] call performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStep {
    /// A fresh commit was recorded and broadcast (`pending → commit_submitted`).
    CommitBroadcast,
    /// A previously recorded commit was re-broadcast on resume (idempotent).
    CommitRebroadcast,
    /// The commit is recorded but not yet spendable; waiting on chain/mempool.
    CommitPending,
    /// The reveal was recorded and broadcast (`commit_submitted → reveal_submitted`).
    RevealBroadcast,
    /// A previously recorded reveal was re-broadcast on resume (idempotent).
    RevealRebroadcast,
    /// The reveal is recorded but below confirmation depth; still waiting.
    RevealPending,
    /// The reveal confirmed; the transfer is `completed`.
    Completed,
    /// Nothing to do (terminal state, or dry-run with no side effects).
    NoChange,
}

/// Aggregate outcome of a [`settle_pending`] sweep.
#[derive(Debug, Clone, Default)]
pub struct SettleReport {
    /// Commits broadcast for the first time.
    pub commits_broadcast: usize,
    /// Reveals broadcast for the first time.
    pub reveals_broadcast: usize,
    /// Re-broadcasts of already-recorded commits/reveals (crash recovery).
    pub rebroadcasts: usize,
    /// Transfers that reached `completed` this sweep.
    pub completed: usize,
    /// Transfers still awaiting inclusion/maturity/confirmation.
    pub pending: usize,
    /// Per-transfer non-fatal errors (one bad transfer never blocks others).
    pub errors: Vec<String>,
}

impl SettleReport {
    const fn record(&mut self, step: TransferStep) {
        match step {
            TransferStep::CommitBroadcast => self.commits_broadcast += 1,
            TransferStep::RevealBroadcast => self.reveals_broadcast += 1,
            TransferStep::CommitRebroadcast | TransferStep::RevealRebroadcast => {
                self.rebroadcasts += 1;
            }
            TransferStep::Completed => self.completed += 1,
            TransferStep::CommitPending | TransferStep::RevealPending => self.pending += 1,
            TransferStep::NoChange => {}
        }
    }
}

/// Failures that abort processing of a single transfer.
#[derive(Debug, thiserror::Error)]
pub enum Krc20ExecuteError {
    /// Database error.
    #[error("db: {0}")]
    Db(#[from] DbError),

    /// kaspad RPC error.
    #[error("kaspad: {0}")]
    Kaspad(#[from] KaspadError),

    /// Planning error (inscription, funding, mass, or sub-floor return).
    #[error("plan: {0}")]
    Plan(#[from] PlanError),

    /// Inscription/address derivation error.
    #[error("inscription: {0}")]
    Inscription(#[from] crate::inscription::InscriptionError),

    /// Signing or post-sign verification error.
    #[error("sign: {0}")]
    Sign(#[from] SignError),

    /// The treasury secret is not a valid secp256k1 key.
    #[error("invalid treasury key")]
    Key(secp256k1::Error),

    /// A persisted amount (NACHO units or commit lock) is negative/out of range.
    #[error("non-representable amount for payout {payout_id}: {field}={value}")]
    Amount {
        /// Parent payout id.
        payout_id: i64,
        /// Offending column.
        field: &'static str,
        /// Stored value.
        value: i64,
    },

    /// A recorded tx hash is not 32 bytes.
    #[error("malformed tx hash on payout {payout_id}")]
    MalformedHash {
        /// Parent payout id.
        payout_id: i64,
    },

    /// The transfer expected a recorded hash that is absent.
    #[error("missing {kind} hash on payout {payout_id}")]
    MissingHash {
        /// Parent payout id.
        payout_id: i64,
        /// `"commit"` or `"reveal"`.
        kind: &'static str,
    },

    /// The stored P2SH address does not match the rebuilt inscription —
    /// configuration drift (wrong key, ticker, amount, or recipient).
    #[error("inscription drift on payout {payout_id}: stored {stored}, rebuilt {rebuilt}")]
    InscriptionMismatch {
        /// Parent payout id.
        payout_id: i64,
        /// P2SH address persisted at planning.
        stored: String,
        /// P2SH address derived from the current inscription.
        rebuilt: String,
    },

    /// On resume, the recorded commit is neither on chain nor reproducible
    /// from the live treasury UTXO set; refuse to broadcast a divergent spend.
    #[error("commit drift on payout {payout_id}: recorded {recorded}, would rebuild {rebuilt}")]
    CommitDrift {
        /// Parent payout id.
        payout_id: i64,
        /// The recorded (already-intended) commit txid.
        recorded: String,
        /// The txid the current UTXO set would produce.
        rebuilt: String,
    },

    /// On resume, the recorded reveal is absent and the reconstructed reveal
    /// would produce a different txid (commit outpoint drift).
    #[error("reveal drift on payout {payout_id}: recorded {recorded}, would rebuild {rebuilt}")]
    RevealDrift {
        /// Parent payout id.
        payout_id: i64,
        /// The recorded reveal txid.
        recorded: String,
        /// The txid the current reconstruction would produce.
        rebuilt: String,
    },
}

/// Everything derived once per transfer and shared by the state handlers.
struct TransferCtx<'a> {
    secret: &'a TreasurySecret,
    treasury_address: &'a Address,
    treasury_script: ScriptPublicKey,
    prefix: Prefix,
    xonly: [u8; 32],
    fees: Krc20FeeConfig,
    inscription: Krc20Transfer,
    commit_amount_sompi: u64,
    payout_id: i64,
    p2sh_address: String,
}

/// Process every actionable transfer (`pending`, `commit_submitted`,
/// `reveal_submitted`) up to `limit`, advancing each at most one state.
///
/// Per-transfer errors are collected into [`SettleReport::errors`] so one bad
/// transfer never blocks the rest; infrastructure errors surface from the
/// initial load.
///
/// # Errors
///
/// [`Krc20ExecuteError::Db`] if the transfer list cannot be loaded.
pub async fn settle_pending<C: KaspadClient + Sync>(
    pool: &PgPool,
    client: &C,
    secret: &TreasurySecret,
    treasury_address: &Address,
    fees: Krc20FeeConfig,
    limit: i64,
    mode: ExecutionMode,
) -> Result<SettleReport, Krc20ExecuteError> {
    let transfers = payout::list_krc20_by_status(
        pool,
        &[
            Krc20TransferStatus::Pending,
            Krc20TransferStatus::CommitSubmitted,
            Krc20TransferStatus::RevealSubmitted,
        ],
        limit,
    )
    .await?;

    let mut report = SettleReport::default();
    for transfer in &transfers {
        match advance_transfer(pool, client, secret, treasury_address, fees, transfer, mode).await {
            Ok(step) => report.record(step),
            Err(e) => report.errors.push(format!(
                "krc20 transfer {} (payout {}): {e}",
                transfer.id, transfer.payout_id
            )),
        }
    }
    Ok(report)
}

/// Advance one transfer by at most one state, performing the chain reads its
/// current status requires.
///
/// # Errors
///
/// See [`Krc20ExecuteError`].
pub async fn advance_transfer<C: KaspadClient + Sync>(
    pool: &PgPool,
    client: &C,
    secret: &TreasurySecret,
    treasury_address: &Address,
    fees: Krc20FeeConfig,
    transfer: &Krc20PendingTransfer,
    mode: ExecutionMode,
) -> Result<TransferStep, Krc20ExecuteError> {
    if matches!(
        transfer.status,
        Krc20TransferStatus::Completed | Krc20TransferStatus::Failed
    ) {
        return Ok(TransferStep::NoChange);
    }

    let keypair = Keypair::from_seckey_slice(secp256k1::SECP256K1, secret.expose_secret())
        .map_err(Krc20ExecuteError::Key)?;
    let xonly = keypair.x_only_public_key().0.serialize();

    let payout_row = payout::get_payout(pool, transfer.payout_id).await?;
    let recipient = wallet::get_by_id(pool, payout_row.wallet_id).await?;

    let nacho_amount =
        u64::try_from(transfer.nacho_amount).map_err(|_| Krc20ExecuteError::Amount {
            payout_id: transfer.payout_id,
            field: "nacho_amount",
            value: transfer.nacho_amount,
        })?;
    let commit_amount_sompi =
        u64::try_from(transfer.sompi_to_miner).map_err(|_| Krc20ExecuteError::Amount {
            payout_id: transfer.payout_id,
            field: "sompi_to_miner",
            value: transfer.sompi_to_miner,
        })?;

    let ctx = TransferCtx {
        secret,
        treasury_address,
        treasury_script: pay_to_address_script(treasury_address),
        prefix: treasury_address.prefix,
        xonly,
        fees,
        inscription: Krc20Transfer::new(NACHO_TICK, nacho_amount.to_string(), recipient.address),
        commit_amount_sompi,
        payout_id: transfer.payout_id,
        p2sh_address: transfer.p2sh_address.clone(),
    };

    match transfer.status {
        Krc20TransferStatus::Pending => handle_pending(pool, client, &ctx, mode).await,
        Krc20TransferStatus::CommitSubmitted => {
            handle_commit_submitted(pool, client, &ctx, &payout_row, mode).await
        }
        Krc20TransferStatus::RevealSubmitted => {
            handle_reveal_submitted(pool, client, &ctx, &payout_row, mode).await
        }
        Krc20TransferStatus::Completed | Krc20TransferStatus::Failed => Ok(TransferStep::NoChange),
    }
}

// ---- state handlers -------------------------------------------------

async fn handle_pending<C: KaspadClient + Sync>(
    pool: &PgPool,
    client: &C,
    ctx: &TransferCtx<'_>,
    mode: ExecutionMode,
) -> Result<TransferStep, Krc20ExecuteError> {
    let plan = build_plan(client, ctx).await?;
    verify_p2sh(ctx, &plan)?;

    let signed = sign_commit(&plan, &ctx.treasury_script, ctx.secret)?;
    let commit_id = signed.txid();

    if matches!(mode, ExecutionMode::DryRun) {
        return Ok(TransferStep::NoChange);
    }

    // Record intent (hash + state) atomically, before the tx hits the wire.
    let mut db = pool.begin().await.map_err(DbError::from)?;
    payout::record_krc20_commit_hash(&mut *db, ctx.payout_id, txid_to_hash(commit_id)).await?;
    payout::mark_krc20_commit_submitted(&mut *db, ctx.payout_id).await?;
    db.commit().await.map_err(DbError::from)?;

    client.submit_transaction(&signed.tx, false).await?;
    Ok(TransferStep::CommitBroadcast)
}

async fn handle_commit_submitted<C: KaspadClient + Sync>(
    pool: &PgPool,
    client: &C,
    ctx: &TransferCtx<'_>,
    payout_row: &Payout,
    mode: ExecutionMode,
) -> Result<TransferStep, Krc20ExecuteError> {
    let recorded = recorded_txid(
        payout_row.krc20_commit_hash.as_deref(),
        ctx.payout_id,
        "commit",
    )?;

    // Reveal-only reconstruction: the redeem script + P2SH address it spends.
    let reveal_plan = PlannedCommitReveal::reveal_only(
        &ctx.xonly,
        &ctx.inscription,
        ctx.commit_amount_sompi,
        ctx.fees.reveal_fee_sompi,
    )?;
    let commit_addr = commit_address(&reveal_plan.redeem_script, ctx.prefix)?;
    let commit_outpoint = TransactionOutpoint {
        transaction_id: recorded,
        index: COMMIT_P2SH_OUTPUT_INDEX,
    };

    let virtual_daa = client.virtual_daa_score().await?;
    let p2sh_utxos = client.treasury_utxos(&commit_addr).await?;
    let on_chain = p2sh_utxos.iter().find(|s| s.outpoint == commit_outpoint);

    match on_chain {
        Some(s) if is_spendable(s.entry.block_daa_score, s.entry.is_coinbase, virtual_daa) => {
            broadcast_reveal(pool, client, ctx, &reveal_plan, commit_outpoint, mode).await
        }
        // On chain but not yet matured — wait.
        Some(_) => Ok(TransferStep::CommitPending),
        None => {
            if client.transaction_in_mempool(recorded).await? {
                return Ok(TransferStep::CommitPending);
            }
            if matches!(mode, ExecutionMode::DryRun) {
                return Ok(TransferStep::NoChange);
            }
            // Neither on chain nor in mempool: crash-before-broadcast (or a
            // dropped mempool entry). Rebuild from live UTXOs; only re-broadcast
            // if it reproduces the recorded txid — otherwise treasury UTXOs
            // drifted and a new commit would be a distinct spend.
            let plan = build_plan(client, ctx).await?;
            let rebuilt = commit_txid(&plan, &ctx.treasury_script)?;
            if rebuilt != recorded {
                return Err(Krc20ExecuteError::CommitDrift {
                    payout_id: ctx.payout_id,
                    recorded: recorded.to_string(),
                    rebuilt: rebuilt.to_string(),
                });
            }
            let signed = sign_commit(&plan, &ctx.treasury_script, ctx.secret)?;
            client.submit_transaction(&signed.tx, false).await?;
            Ok(TransferStep::CommitRebroadcast)
        }
    }
}

async fn handle_reveal_submitted<C: KaspadClient + Sync>(
    pool: &PgPool,
    client: &C,
    ctx: &TransferCtx<'_>,
    payout_row: &Payout,
    mode: ExecutionMode,
) -> Result<TransferStep, Krc20ExecuteError> {
    let reveal_recorded = recorded_txid(
        payout_row.krc20_reveal_hash.as_deref(),
        ctx.payout_id,
        "reveal",
    )?;

    let virtual_daa = client.virtual_daa_score().await?;
    let treasury_utxos = client.treasury_utxos(ctx.treasury_address).await?;
    // The reveal's return coin lands at the treasury bearing the reveal txid.
    let on_chain_daa = treasury_utxos
        .iter()
        .find(|s| s.outpoint.transaction_id == reveal_recorded)
        .map(|s| s.entry.block_daa_score);
    let in_mempool = if on_chain_daa.is_some() {
        false
    } else {
        client.transaction_in_mempool(reveal_recorded).await?
    };

    let state = classify_confirmation(ConfirmationInputs {
        virtual_daa_score: virtual_daa,
        in_mempool,
        change_block_daa_score: on_chain_daa,
    });

    match state {
        ConfirmationState::Confirmed => {
            if matches!(mode, ExecutionMode::DryRun) {
                return Ok(TransferStep::NoChange);
            }
            payout::mark_krc20_completed(pool, ctx.payout_id).await?;
            Ok(TransferStep::Completed)
        }
        ConfirmationState::Accepted | ConfirmationState::Pending => Ok(TransferStep::RevealPending),
        ConfirmationState::Unknown => {
            if matches!(mode, ExecutionMode::DryRun) {
                return Ok(TransferStep::NoChange);
            }
            // Reveal absent from chain and mempool: crash-before-broadcast.
            // Rebuild deterministically from the recorded commit outpoint.
            let commit_recorded = recorded_txid(
                payout_row.krc20_commit_hash.as_deref(),
                ctx.payout_id,
                "commit",
            )?;
            let commit_outpoint = TransactionOutpoint {
                transaction_id: commit_recorded,
                index: COMMIT_P2SH_OUTPUT_INDEX,
            };
            let reveal_plan = PlannedCommitReveal::reveal_only(
                &ctx.xonly,
                &ctx.inscription,
                ctx.commit_amount_sompi,
                ctx.fees.reveal_fee_sompi,
            )?;
            let rebuilt = reveal_txid(&reveal_plan, commit_outpoint, &ctx.treasury_script);
            if rebuilt != reveal_recorded {
                return Err(Krc20ExecuteError::RevealDrift {
                    payout_id: ctx.payout_id,
                    recorded: reveal_recorded.to_string(),
                    rebuilt: rebuilt.to_string(),
                });
            }
            let signed = sign_reveal(
                &reveal_plan,
                commit_outpoint,
                &ctx.treasury_script,
                ctx.secret,
            )?;
            client.submit_transaction(&signed.tx, false).await?;
            Ok(TransferStep::RevealRebroadcast)
        }
    }
}

/// Record the reveal intent atomically, then broadcast it.
async fn broadcast_reveal<C: KaspadClient + Sync>(
    pool: &PgPool,
    client: &C,
    ctx: &TransferCtx<'_>,
    reveal_plan: &PlannedCommitReveal,
    commit_outpoint: TransactionOutpoint,
    mode: ExecutionMode,
) -> Result<TransferStep, Krc20ExecuteError> {
    let signed = sign_reveal(
        reveal_plan,
        commit_outpoint,
        &ctx.treasury_script,
        ctx.secret,
    )?;
    let reveal_id = signed.txid();

    if matches!(mode, ExecutionMode::DryRun) {
        return Ok(TransferStep::NoChange);
    }

    let mut db = pool.begin().await.map_err(DbError::from)?;
    payout::record_krc20_reveal_hash(&mut *db, ctx.payout_id, txid_to_hash(reveal_id)).await?;
    payout::mark_krc20_reveal_submitted(&mut *db, ctx.payout_id).await?;
    db.commit().await.map_err(DbError::from)?;

    client.submit_transaction(&signed.tx, false).await?;
    Ok(TransferStep::RevealBroadcast)
}

// ---- helpers --------------------------------------------------------

/// Plan a fresh commit/reveal against the live, spendable treasury UTXO set.
async fn build_plan<C: KaspadClient + Sync>(
    client: &C,
    ctx: &TransferCtx<'_>,
) -> Result<PlannedCommitReveal, Krc20ExecuteError> {
    let virtual_daa = client.virtual_daa_score().await?;
    let snapshots = client.treasury_utxos(ctx.treasury_address).await?;
    let utxos: Vec<TreasuryUtxo> = snapshots
        .into_iter()
        .filter(|s| is_spendable(s.entry.block_daa_score, s.entry.is_coinbase, virtual_daa))
        .map(TreasuryUtxoSnapshot::into_treasury_utxo)
        .collect();

    let cfg = CommitRevealConfig {
        commit_amount_sompi: ctx.commit_amount_sompi,
        commit_fee_sompi: ctx.fees.commit_fee_sompi,
        reveal_fee_sompi: ctx.fees.reveal_fee_sompi,
    };
    let evaluator = MassEvaluator::mainnet();
    let plan = plan_commit_reveal(
        &evaluator,
        &utxos,
        &ctx.treasury_script,
        &ctx.xonly,
        &ctx.inscription,
        &cfg,
    )?;
    Ok(plan)
}

/// Guard against inscription/config drift: the P2SH the plan pays must equal
/// the address persisted when the transfer was first planned.
fn verify_p2sh(ctx: &TransferCtx<'_>, plan: &PlannedCommitReveal) -> Result<(), Krc20ExecuteError> {
    let rebuilt = commit_address(&plan.redeem_script, ctx.prefix)?.to_string();
    if rebuilt != ctx.p2sh_address {
        return Err(Krc20ExecuteError::InscriptionMismatch {
            payout_id: ctx.payout_id,
            stored: ctx.p2sh_address.clone(),
            rebuilt,
        });
    }
    Ok(())
}

const fn txid_to_hash(id: TransactionId) -> BlockHash {
    BlockHash::from_bytes(id.as_bytes())
}

fn recorded_txid(
    hash: Option<&[u8]>,
    payout_id: i64,
    kind: &'static str,
) -> Result<TransactionId, Krc20ExecuteError> {
    let bytes = hash.ok_or(Krc20ExecuteError::MissingHash { payout_id, kind })?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Krc20ExecuteError::MalformedHash { payout_id })?;
    Ok(TransactionId::from_bytes(arr))
}
