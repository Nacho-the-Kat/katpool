//! Payout aggregates — cycle, individual payout, and the KRC-20
//! commit/reveal state machine.
//!
//! All three tables share a strict idempotency story: each cycle has
//! a human-readable `idempotency_key` (`kas-<daa_start>-<daa_end>` or
//! `krc20-<daa_start>-<daa_end>`) and each payout has the
//! `UNIQUE (cycle_id, wallet_id)` guard. Retrying a broadcast that
//! partially succeeded is `INSERT ON CONFLICT DO NOTHING` for the
//! cycle and a no-op for already-existing payout rows.

// daa_start/daa_end columns are BIGINT-bounded by chain reality.
#![allow(clippy::cast_possible_wrap)]

use chrono::{DateTime, Utc};
use katpool_domain::{BlockHash, DaaScore};
use sqlx::PgExecutor;

use crate::DbError;
use crate::repo::WalletId;

// ---- enums ----------------------------------------------------------

/// Kind of payout cycle. Mirrors the `payout_kind` Postgres enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "payout_kind", rename_all = "snake_case")]
pub enum PayoutKind {
    /// KAS payout cycle (native KAS transactions).
    Kas,
    /// KRC-20 NACHO payout cycle (commit/reveal pair per recipient).
    Krc20Nacho,
}

/// Cycle status state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "payout_cycle_status", rename_all = "snake_case")]
pub enum PayoutCycleStatus {
    /// Allocations computed; transactions not yet broadcast.
    Planned,
    /// Transactions in flight on the wire.
    Broadcasting,
    /// Some recipients confirmed, others still pending.
    PartiallySettled,
    /// Every recipient confirmed on chain.
    Settled,
    /// Broadcast errored; needs investigation.
    Failed,
}

/// Per-recipient payout status state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "payout_status", rename_all = "snake_case")]
pub enum PayoutStatus {
    /// Cycle allocation produced this row; no tx yet.
    Planned,
    /// Transaction submitted to mempool.
    Submitted,
    /// Transaction accepted by network (first confirmation).
    Accepted,
    /// Confirmed past maturity window.
    Confirmed,
    /// Transaction failed; `failure_reason` carries detail.
    Failed,
}

/// KRC-20 transfer status state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "krc20_transfer_status", rename_all = "snake_case")]
pub enum Krc20TransferStatus {
    /// Commit transaction not yet submitted.
    Pending,
    /// Commit tx submitted (on chain).
    CommitSubmitted,
    /// Reveal tx submitted (on chain).
    RevealSubmitted,
    /// Both commit and reveal confirmed.
    Completed,
    /// Transfer failed irrecoverably.
    Failed,
}

// ---- rows -----------------------------------------------------------

/// One row of `payout_cycle`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PayoutCycle {
    /// Synthetic primary key.
    pub id: i64,
    /// Cycle kind.
    pub kind: PayoutKind,
    /// Status state.
    pub status: PayoutCycleStatus,
    /// Half-open DAA range start (inclusive).
    pub daa_start: i64,
    /// Half-open DAA range end (exclusive).
    pub daa_end: i64,
    /// When the cycle row was created.
    pub planned_at: DateTime<Utc>,
    /// When the broadcast started.
    pub broadcast_at: Option<DateTime<Utc>>,
    /// When the last recipient confirmed.
    pub settled_at: Option<DateTime<Utc>>,
    /// Sum of payout amounts across all recipients in the cycle.
    pub total_sompi: i64,
    /// Number of recipients in the cycle.
    pub total_recipients: i32,
    /// Human-readable idempotency key.
    pub idempotency_key: String,
}

/// One row of `payout`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Payout {
    /// Synthetic primary key.
    pub id: i64,
    /// FK to `payout_cycle.id`.
    pub cycle_id: i64,
    /// FK to `wallet.id`.
    pub wallet_id: WalletId,
    /// Payout amount in sompi.
    pub amount_sompi: i64,
    /// Status state.
    pub status: PayoutStatus,
    /// KAS tx hash; populated for `Kas` cycle on submit.
    pub tx_hash: Option<Vec<u8>>,
    /// KRC-20 commit tx hash; populated on `commit_submitted`.
    pub krc20_commit_hash: Option<Vec<u8>>,
    /// KRC-20 reveal tx hash; populated on `reveal_submitted`.
    pub krc20_reveal_hash: Option<Vec<u8>>,
    /// When the payout row was created.
    pub planned_at: DateTime<Utc>,
    /// When the tx was submitted.
    pub submitted_at: Option<DateTime<Utc>>,
    /// When the tx was confirmed past maturity.
    pub confirmed_at: Option<DateTime<Utc>>,
    /// Why the payout failed (if `status = Failed`).
    pub failure_reason: Option<String>,
}

/// One row of `krc20_pending_transfer`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Krc20PendingTransfer {
    /// Synthetic primary key.
    pub id: i64,
    /// FK to `payout.id`.
    pub payout_id: i64,
    /// KAS sompi included in the commit tx (covers tx fees on reveal).
    pub sompi_to_miner: i64,
    /// NACHO integer-unit amount.
    pub nacho_amount: i64,
    /// P2SH address derived from the commit script.
    pub p2sh_address: String,
    /// State.
    pub status: Krc20TransferStatus,
    /// Created-at timestamp.
    pub created_at: DateTime<Utc>,
    /// Updated-at timestamp.
    pub updated_at: DateTime<Utc>,
}

// ---- cycle ops ------------------------------------------------------

/// Compose the cycle's idempotency key from its (kind, daa range).
#[must_use]
pub fn idempotency_key(kind: PayoutKind, daa_start: DaaScore, daa_end: DaaScore) -> String {
    let prefix = match kind {
        PayoutKind::Kas => "kas",
        PayoutKind::Krc20Nacho => "krc20",
    };
    format!("{prefix}-{}-{}", daa_start.value(), daa_end.value())
}

/// Create a cycle. Idempotent via the unique `idempotency_key`
/// column — calling twice with the same key returns the existing
/// row without conflict noise.
pub async fn create_cycle<'e, E>(
    executor: E,
    kind: PayoutKind,
    daa_start: DaaScore,
    daa_end: DaaScore,
) -> Result<PayoutCycle, DbError>
where
    E: PgExecutor<'e>,
{
    let key = idempotency_key(kind, daa_start, daa_end);
    sqlx::query_as::<_, PayoutCycle>(
        "INSERT INTO payout_cycle (kind, daa_start, daa_end, idempotency_key)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (idempotency_key) DO UPDATE
            SET idempotency_key = EXCLUDED.idempotency_key
         RETURNING id, kind, status, daa_start, daa_end, planned_at, broadcast_at,
                   settled_at, total_sompi, total_recipients, idempotency_key",
    )
    .bind(kind)
    .bind(daa_start.value() as i64)
    .bind(daa_end.value() as i64)
    .bind(key)
    .fetch_one(executor)
    .await
    .map_err(DbError::from)
}

/// Look up a cycle by its idempotency key.
pub async fn find_cycle_by_idempotency_key<'e, E: PgExecutor<'e>>(
    executor: E,
    key: &str,
) -> Result<Option<PayoutCycle>, DbError> {
    sqlx::query_as::<_, PayoutCycle>(
        "SELECT id, kind, status, daa_start, daa_end, planned_at, broadcast_at,
                settled_at, total_sompi, total_recipients, idempotency_key
           FROM payout_cycle
          WHERE idempotency_key = $1",
    )
    .bind(key)
    .fetch_optional(executor)
    .await
    .map_err(DbError::from)
}

/// Get a cycle by primary key.
pub async fn get_cycle<'e, E: PgExecutor<'e>>(
    executor: E,
    cycle_id: i64,
) -> Result<PayoutCycle, DbError> {
    sqlx::query_as::<_, PayoutCycle>(
        "SELECT id, kind, status, daa_start, daa_end, planned_at, broadcast_at,
                settled_at, total_sompi, total_recipients, idempotency_key
           FROM payout_cycle
          WHERE id = $1",
    )
    .bind(cycle_id)
    .fetch_one(executor)
    .await
    .map_err(DbError::from)
}

/// Advance a cycle to `broadcasting`. Idempotent.
pub async fn mark_cycle_broadcasting<'e, E: PgExecutor<'e>>(
    executor: E,
    cycle_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE payout_cycle
            SET status = 'broadcasting',
                broadcast_at = COALESCE(broadcast_at, now())
          WHERE id = $1
            AND status IN ('planned', 'broadcasting')",
    )
    .bind(cycle_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Advance a cycle to `partially_settled`.
pub async fn mark_cycle_partially_settled<'e, E: PgExecutor<'e>>(
    executor: E,
    cycle_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE payout_cycle
            SET status = 'partially_settled'
          WHERE id = $1
            AND status IN ('broadcasting', 'partially_settled')",
    )
    .bind(cycle_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Advance a cycle to `settled`.
pub async fn mark_cycle_settled<'e, E: PgExecutor<'e>>(
    executor: E,
    cycle_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE payout_cycle
            SET status = 'settled',
                settled_at = COALESCE(settled_at, now())
          WHERE id = $1
            AND status IN ('broadcasting', 'partially_settled', 'settled')",
    )
    .bind(cycle_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Mark a cycle failed. Records a reason via the audit log; callers
/// should pair this with `repo::audit::append` for forensic detail.
pub async fn mark_cycle_failed<'e, E: PgExecutor<'e>>(
    executor: E,
    cycle_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE payout_cycle
            SET status = 'failed'
          WHERE id = $1
            AND status <> 'settled'",
    )
    .bind(cycle_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Update the cycle totals after the planning step finalises
/// recipients.
pub async fn set_cycle_totals<'e, E: PgExecutor<'e>>(
    executor: E,
    cycle_id: i64,
    total_sompi: i64,
    total_recipients: i32,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE payout_cycle
            SET total_sompi = $2,
                total_recipients = $3
          WHERE id = $1",
    )
    .bind(cycle_id)
    .bind(total_sompi)
    .bind(total_recipients)
    .execute(executor)
    .await?;
    Ok(())
}

// ---- payout ops -----------------------------------------------------

/// Insert a planned payout for a recipient under a cycle.
///
/// Idempotent: the `UNIQUE (cycle_id, wallet_id)` guard rejects
/// duplicates with SQLSTATE `23505`. Callers can either pre-filter
/// or treat the constraint error as a no-op.
pub async fn insert_payout<'e, E>(
    executor: E,
    cycle_id: i64,
    wallet_id: WalletId,
    amount_sompi: i64,
) -> Result<Payout, DbError>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as::<_, Payout>(
        "INSERT INTO payout (cycle_id, wallet_id, amount_sompi)
         VALUES ($1, $2, $3)
         RETURNING id, cycle_id, wallet_id, amount_sompi, status, tx_hash,
                   krc20_commit_hash, krc20_reveal_hash, planned_at, submitted_at,
                   confirmed_at, failure_reason",
    )
    .bind(cycle_id)
    .bind(wallet_id.0)
    .bind(amount_sompi)
    .fetch_one(executor)
    .await
    .map_err(DbError::from)
}

/// Get a payout by primary key.
pub async fn get_payout<'e, E: PgExecutor<'e>>(
    executor: E,
    payout_id: i64,
) -> Result<Payout, DbError> {
    sqlx::query_as::<_, Payout>(
        "SELECT id, cycle_id, wallet_id, amount_sompi, status, tx_hash,
                krc20_commit_hash, krc20_reveal_hash, planned_at, submitted_at,
                confirmed_at, failure_reason
           FROM payout
          WHERE id = $1",
    )
    .bind(payout_id)
    .fetch_one(executor)
    .await
    .map_err(DbError::from)
}

/// List every payout under a cycle.
pub async fn list_for_cycle<'e, E: PgExecutor<'e>>(
    executor: E,
    cycle_id: i64,
) -> Result<Vec<Payout>, DbError> {
    sqlx::query_as::<_, Payout>(
        "SELECT id, cycle_id, wallet_id, amount_sompi, status, tx_hash,
                krc20_commit_hash, krc20_reveal_hash, planned_at, submitted_at,
                confirmed_at, failure_reason
           FROM payout
          WHERE cycle_id = $1
          ORDER BY amount_sompi DESC, id ASC",
    )
    .bind(cycle_id)
    .fetch_all(executor)
    .await
    .map_err(DbError::from)
}

/// Recent payouts for one wallet.
pub async fn list_for_wallet<'e, E: PgExecutor<'e>>(
    executor: E,
    wallet_id: WalletId,
    limit: i64,
) -> Result<Vec<Payout>, DbError> {
    sqlx::query_as::<_, Payout>(
        "SELECT id, cycle_id, wallet_id, amount_sompi, status, tx_hash,
                krc20_commit_hash, krc20_reveal_hash, planned_at, submitted_at,
                confirmed_at, failure_reason
           FROM payout
          WHERE wallet_id = $1
          ORDER BY planned_at DESC
          LIMIT $2",
    )
    .bind(wallet_id.0)
    .bind(limit)
    .fetch_all(executor)
    .await
    .map_err(DbError::from)
}

/// Mark a KAS payout submitted, recording the on-chain tx hash.
pub async fn mark_payout_submitted<'e, E: PgExecutor<'e>>(
    executor: E,
    payout_id: i64,
    tx_hash: BlockHash,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE payout
            SET status = 'submitted',
                tx_hash = $2,
                submitted_at = COALESCE(submitted_at, now())
          WHERE id = $1
            AND status IN ('planned', 'submitted')",
    )
    .bind(payout_id)
    .bind(tx_hash.as_bytes().to_vec())
    .execute(executor)
    .await?;
    Ok(())
}

/// Mark a payout confirmed past maturity window.
pub async fn mark_payout_confirmed<'e, E: PgExecutor<'e>>(
    executor: E,
    payout_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE payout
            SET status = 'confirmed',
                confirmed_at = COALESCE(confirmed_at, now())
          WHERE id = $1
            AND status IN ('submitted', 'accepted', 'confirmed')",
    )
    .bind(payout_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Mark a payout failed with a reason. Failed payouts can be retried
/// by re-planning the recipient in a fresh cycle.
pub async fn mark_payout_failed<'e, E: PgExecutor<'e>>(
    executor: E,
    payout_id: i64,
    reason: &str,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE payout
            SET status = 'failed',
                failure_reason = $2
          WHERE id = $1
            AND status <> 'confirmed'",
    )
    .bind(payout_id)
    .bind(reason)
    .execute(executor)
    .await?;
    Ok(())
}

// ---- KRC-20 ops -----------------------------------------------------

/// Open a new pending KRC-20 transfer associated with a payout row.
/// One-to-one with the parent payout.
pub async fn insert_krc20_pending<'e, E>(
    executor: E,
    payout_id: i64,
    sompi_to_miner: i64,
    nacho_amount: i64,
    p2sh_address: &str,
) -> Result<Krc20PendingTransfer, DbError>
where
    E: PgExecutor<'e>,
{
    sqlx::query_as::<_, Krc20PendingTransfer>(
        "INSERT INTO krc20_pending_transfer
            (payout_id, sompi_to_miner, nacho_amount, p2sh_address)
         VALUES ($1, $2, $3, $4)
         RETURNING id, payout_id, sompi_to_miner, nacho_amount, p2sh_address,
                   status, created_at, updated_at",
    )
    .bind(payout_id)
    .bind(sompi_to_miner)
    .bind(nacho_amount)
    .bind(p2sh_address)
    .fetch_one(executor)
    .await
    .map_err(DbError::from)
}

/// Advance a transfer to `commit_submitted`.
pub async fn mark_krc20_commit_submitted<'e, E: PgExecutor<'e>>(
    executor: E,
    payout_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE krc20_pending_transfer
            SET status = 'commit_submitted',
                updated_at = now()
          WHERE payout_id = $1
            AND status IN ('pending', 'commit_submitted')",
    )
    .bind(payout_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Advance a transfer to `reveal_submitted`.
pub async fn mark_krc20_reveal_submitted<'e, E: PgExecutor<'e>>(
    executor: E,
    payout_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE krc20_pending_transfer
            SET status = 'reveal_submitted',
                updated_at = now()
          WHERE payout_id = $1
            AND status IN ('commit_submitted', 'reveal_submitted')",
    )
    .bind(payout_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Advance a transfer to `completed`.
pub async fn mark_krc20_completed<'e, E: PgExecutor<'e>>(
    executor: E,
    payout_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE krc20_pending_transfer
            SET status = 'completed',
                updated_at = now()
          WHERE payout_id = $1
            AND status IN ('reveal_submitted', 'completed')",
    )
    .bind(payout_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// Mark a transfer failed.
pub async fn mark_krc20_failed<'e, E: PgExecutor<'e>>(
    executor: E,
    payout_id: i64,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE krc20_pending_transfer
            SET status = 'failed',
                updated_at = now()
          WHERE payout_id = $1
            AND status <> 'completed'",
    )
    .bind(payout_id)
    .execute(executor)
    .await?;
    Ok(())
}

/// List KRC-20 transfers in any of the given statuses.
pub async fn list_krc20_by_status<'e, E: PgExecutor<'e>>(
    executor: E,
    statuses: &[Krc20TransferStatus],
    limit: i64,
) -> Result<Vec<Krc20PendingTransfer>, DbError> {
    sqlx::query_as::<_, Krc20PendingTransfer>(
        "SELECT id, payout_id, sompi_to_miner, nacho_amount, p2sh_address,
                status, created_at, updated_at
           FROM krc20_pending_transfer
          WHERE status = ANY($1)
          ORDER BY created_at ASC
          LIMIT $2",
    )
    .bind(statuses)
    .bind(limit)
    .fetch_all(executor)
    .await
    .map_err(DbError::from)
}
