//! `pending_krc20_transfers` → `krc20_pending_transfer` transform.
//!
//! Every legacy row creates a synthetic singleton `payout_cycle`
//! (`kind=krc20_nacho`), a `payout` row under it, and the
//! `krc20_pending_transfer` row linked by FK. Singleton because the
//! legacy table didn't batch transfers across recipients — each
//! row was its own pending transfer.
//!
//! Status mapping:
//!
//! | Legacy `nacho_transfer_status` | New `krc20_transfer_status` |
//! |---|---|
//! | `PENDING` | `pending` |
//! | `COMPLETED` | `completed` |
//! | `FAILED` | `failed` |
//!
//! Legacy never modelled the intermediate states
//! (`commit_submitted`, `reveal_submitted`) — those only exist in
//! the new schema.

#![allow(
    clippy::cast_possible_wrap,
    clippy::cognitive_complexity,
    clippy::explicit_auto_deref,
    clippy::single_match_else
)]

use chrono::{DateTime, NaiveDateTime, Utc};
use katpool_db::repo::payout::{self, Krc20TransferStatus, PayoutKind};
use katpool_db::repo::{WalletId, wallet};
use katpool_domain::WalletAddress;
use tracing::{info, warn};

use crate::source::{self, LegacyKrc20Transfer};
use crate::transform::TransformStats;

const LEGACY_NETWORK: &str = "mainnet";

/// Run the `pending_krc20_transfers` →
/// `payout_cycle`+`payout`+`krc20_pending_transfer` transform.
/// Rejected `pending_krc20_transfers` rows by legacy status.
///
/// Lets the reconcile's per-status count checks tolerate exactly the rows the
/// importer dropped as invalid.
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusRejects {
    /// Rejected rows whose legacy `nacho_transfer_status` is `PENDING`.
    pub pending: i64,
    /// Rejected rows in `COMPLETED` state.
    pub completed: i64,
    /// Rejected rows in `FAILED` state.
    pub failed: i64,
}

/// Run the `pending_krc20_transfers` → payout transform.
///
/// Returns the per-transform stats and the rejected-row counts by legacy status
/// (for the reconcile's per-status allowance).
pub async fn run(
    source: &sqlx::PgPool,
    target: &sqlx::PgPool,
    dry_run: bool,
) -> Result<(TransformStats, StatusRejects), anyhow::Error> {
    let rows = source::fetch_krc20_transfers(source).await?;
    info!(
        row_count = rows.len(),
        dry_run, "starting pending_krc20_transfers import"
    );

    let mut stats = TransformStats::default();
    let mut rejects = StatusRejects::default();
    for row in rows {
        stats.read += 1;
        match import_one(target, &row, dry_run).await {
            Ok(Outcome::Inserted) => stats.inserted += 1,
            Ok(Outcome::Skipped) => stats.skipped += 1,
            Ok(Outcome::Rejected(reason)) => {
                stats.rejected += 1;
                // Attribute the reject to its legacy status for the reconcile.
                match row.nacho_transfer_status.as_deref() {
                    Some("COMPLETED") => rejects.completed += 1,
                    Some("FAILED") => rejects.failed += 1,
                    _ => rejects.pending += 1,
                }
                warn!(id = row.id, reason, "pending_krc20_transfers row rejected");
            }
            Err(e) => return Err(e.context(format!("import krc20 id={}", row.id))),
        }
    }

    info!(stats = %stats, "pending_krc20_transfers import complete");
    Ok((stats, rejects))
}

#[derive(Debug)]
enum Outcome {
    Inserted,
    Skipped,
    Rejected(&'static str),
}

async fn import_one(
    target: &sqlx::PgPool,
    row: &LegacyKrc20Transfer,
    dry_run: bool,
) -> Result<Outcome, anyhow::Error> {
    let Ok(wallet_addr) = WalletAddress::new(row.address.clone()) else {
        return Ok(Outcome::Rejected("address fails domain validation"));
    };
    if row.sompi_to_miner <= 0 {
        return Ok(Outcome::Rejected("sompi_to_miner must be > 0"));
    }
    if row.nacho_amount <= 0 {
        return Ok(Outcome::Rejected("nacho_amount must be > 0"));
    }

    if dry_run {
        return Ok(Outcome::Inserted);
    }

    // Per-row synthetic cycle; the legacy schema didn't batch these.
    let key = format!("krc20-legacy-pending-{}", row.first_txn_id);
    let new_status = map_status(row.nacho_transfer_status.as_deref());

    let mut tx = target.begin().await?;
    let w = wallet::ensure(&mut *tx, &wallet_addr, LEGACY_NETWORK).await?;
    let cycle = create_legacy_cycle(&mut *tx, PayoutKind::Krc20Nacho, &key).await?;
    let Some(payout_id) = insert_payout(
        &mut *tx,
        cycle.id,
        w.id,
        row.nacho_amount,
        row.timestamp,
        new_status,
    )
    .await?
    else {
        tx.commit().await?;
        return Ok(Outcome::Skipped);
    };
    insert_krc20_pending(&mut *tx, payout_id, row, new_status).await?;
    payout::set_cycle_totals(&mut *tx, cycle.id, row.nacho_amount, 1).await?;
    advance_cycle_for_status(&mut *tx, cycle.id, new_status).await?;
    tx.commit().await?;
    Ok(Outcome::Inserted)
}

async fn insert_payout(
    tx: &mut sqlx::PgConnection,
    cycle_id: i64,
    wallet_id: WalletId,
    amount: i64,
    legacy_ts: Option<NaiveDateTime>,
    transfer_status: Krc20TransferStatus,
) -> Result<Option<i64>, anyhow::Error> {
    let new_payout_status: &str = match transfer_status {
        Krc20TransferStatus::Completed => "confirmed",
        Krc20TransferStatus::Failed => "failed",
        _ => "submitted",
    };
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO payout
            (cycle_id, wallet_id, amount_sompi, status, planned_at, submitted_at, confirmed_at, failure_reason)
         VALUES ($1, $2, $3, $4::payout_status, $5, $5,
                 CASE WHEN $4::payout_status = 'confirmed' THEN $5 END,
                 CASE WHEN $4::payout_status = 'failed' THEN 'imported from legacy pending_krc20_transfers in FAILED state' END)
         ON CONFLICT (cycle_id, wallet_id) DO NOTHING
         RETURNING id",
    )
    .bind(cycle_id)
    .bind(wallet_id.0)
    .bind(amount)
    .bind(new_payout_status)
    .bind(legacy_ts_to_utc(legacy_ts))
    .fetch_optional(&mut *tx)
    .await?;
    Ok(id)
}

async fn insert_krc20_pending(
    tx: &mut sqlx::PgConnection,
    payout_id: i64,
    row: &LegacyKrc20Transfer,
    status: Krc20TransferStatus,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "INSERT INTO krc20_pending_transfer
            (payout_id, sompi_to_miner, nacho_amount, p2sh_address, status, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $6)
         ON CONFLICT (payout_id) DO NOTHING",
    )
    .bind(payout_id)
    .bind(row.sompi_to_miner)
    .bind(row.nacho_amount)
    .bind(&row.p2sh_address)
    .bind(status)
    .bind(legacy_ts_to_utc(row.timestamp))
    .execute(&mut *tx)
    .await?;
    Ok(())
}

async fn advance_cycle_for_status(
    tx: &mut sqlx::PgConnection,
    cycle_id: i64,
    status: Krc20TransferStatus,
) -> Result<(), anyhow::Error> {
    match status {
        Krc20TransferStatus::Pending => Ok(()),
        Krc20TransferStatus::Failed => {
            payout::mark_cycle_failed(&mut *tx, cycle_id).await?;
            Ok(())
        }
        _ => {
            payout::mark_cycle_broadcasting(&mut *tx, cycle_id).await?;
            payout::mark_cycle_settled(&mut *tx, cycle_id).await?;
            Ok(())
        }
    }
}

fn map_status(legacy: Option<&str>) -> Krc20TransferStatus {
    match legacy {
        Some("COMPLETED") => Krc20TransferStatus::Completed,
        Some("FAILED") => Krc20TransferStatus::Failed,
        // PENDING, NULL, or anything else conservative-maps to pending.
        _ => Krc20TransferStatus::Pending,
    }
}

async fn create_legacy_cycle(
    tx: &mut sqlx::PgConnection,
    kind: PayoutKind,
    idempotency_key: &str,
) -> Result<katpool_db::repo::payout::PayoutCycle, anyhow::Error> {
    let cycle = sqlx::query_as::<_, katpool_db::repo::payout::PayoutCycle>(
        "INSERT INTO payout_cycle (kind, daa_start, daa_end, idempotency_key)
         VALUES ($1, 0, 1, $2)
         ON CONFLICT (idempotency_key) DO UPDATE
             SET idempotency_key = EXCLUDED.idempotency_key
         RETURNING id, kind, status, daa_start, daa_end, planned_at, broadcast_at,
                   settled_at, total_sompi, total_recipients, idempotency_key",
    )
    .bind(kind)
    .bind(idempotency_key)
    .fetch_one(&mut *tx)
    .await?;
    Ok(cycle)
}

fn legacy_ts_to_utc(ts: Option<NaiveDateTime>) -> DateTime<Utc> {
    ts.map_or_else(Utc::now, |t| t.and_utc())
}
