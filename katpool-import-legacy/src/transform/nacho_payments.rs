//! `nacho_payments` → `payout_cycle` (`kind=krc20_nacho`) + `payout`
//! transform.
//!
//! Identical shape to [`crate::transform::payments`] but the
//! per-row amount column is `nacho_amount` (not `amount`), the
//! cycle kind is `krc20_nacho`, and the idempotency-key prefix is
//! `krc20-legacy-`.
//!
//! Note that the legacy `nacho_amount` column is denominated in
//! integer NACHO units (not sompi) — but the new `payout.amount_sompi`
//! column is labelled "sompi" in the schema for cross-kind
//! symmetry. We store the legacy NACHO amount in that same `bigint`
//! column with the same units; the column name is generic enough
//! that this isn't lossy. The `payout_kind` enum disambiguates.

// See `payments.rs` for the same rationale on each of these
// monetary/integer-arithmetic lints.
#![allow(
    clippy::cast_possible_wrap,
    clippy::integer_division,
    clippy::cognitive_complexity,
    clippy::explicit_auto_deref
)]

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, NaiveDateTime, Utc};
use katpool_db::repo::payout::{self, PayoutKind};
use katpool_db::repo::{WalletId, wallet};
use katpool_domain::{BlockHash, WalletAddress};
use tracing::{info, warn};

use crate::source::{self, LegacyNachoPayment};
use crate::transform::TransformStats;

const LEGACY_NETWORK: &str = "mainnet";

/// Run the `nacho_payments` → `payout_cycle`/`payout` transform.
pub async fn run(
    source: &sqlx::PgPool,
    target: &sqlx::PgPool,
    dry_run: bool,
) -> Result<TransformStats, anyhow::Error> {
    let rows = source::fetch_nacho_payments(source).await?;
    info!(
        row_count = rows.len(),
        dry_run, "starting nacho_payments import"
    );

    let mut stats = TransformStats::default();
    let mut groups: BTreeMap<String, Vec<LegacyNachoPayment>> = BTreeMap::new();
    for row in rows {
        groups
            .entry(row.transaction_hash.clone())
            .or_default()
            .push(row);
    }
    info!(
        unique_cycles = groups.len(),
        "nacho_payments grouped into cycles"
    );

    for (tx_hash, group) in groups {
        stats.read += group.len() as u64;
        match import_group(target, &tx_hash, &group, dry_run).await {
            Ok(g) => {
                stats.inserted += g.inserted;
                stats.skipped += g.skipped;
                stats.rejected += g.rejected;
                stats.rejected_amount = stats.rejected_amount.saturating_add(g.rejected_amount);
                stats.deduped_amount = stats.deduped_amount.saturating_add(g.deduped_amount);
            }
            Err(e) => {
                return Err(e.context(format!("import nacho_payments cycle tx_hash={tx_hash}")));
            }
        }
    }

    info!(stats = %stats, "nacho_payments import complete");
    Ok(stats)
}

#[derive(Debug, Default, Clone, Copy)]
struct GroupStats {
    inserted: u64,
    skipped: u64,
    rejected: u64,
    /// NACHO base units over rejected rows (invalid wallet/tx/amount).
    rejected_amount: i64,
    /// NACHO base units collapsed by a within-cycle duplicate wallet.
    deduped_amount: i64,
}

async fn import_group(
    target: &sqlx::PgPool,
    tx_hash: &str,
    group: &[LegacyNachoPayment],
    dry_run: bool,
) -> Result<GroupStats, anyhow::Error> {
    let mut stats = GroupStats::default();
    let Some(tx_hash_bytes) = parse_tx_hash(tx_hash) else {
        stats.rejected += group.len() as u64;
        stats.rejected_amount = group
            .iter()
            .fold(0_i64, |acc, r| acc.saturating_add(r.nacho_amount));
        warn!(
            tx_hash,
            "nacho_payments cycle rejected: tx_hash not 64-char hex"
        );
        return Ok(stats);
    };

    let key = format!("krc20-legacy-{tx_hash}");

    if dry_run {
        for row in group {
            stats = classify_dry_run(row, stats);
        }
        return Ok(stats);
    }

    let mut tx = target.begin().await?;
    let cycle = create_legacy_cycle(&mut *tx, PayoutKind::Krc20Nacho, &key).await?;

    let mut cycle_total: i64 = 0;
    let mut cycle_recipients: i32 = 0;
    let earliest_ts = group.iter().filter_map(|r| r.timestamp).min();

    // Wallets credited in this cycle this run — see payments.rs for why the
    // dedup is tracked here rather than via ON CONFLICT (run-stability).
    let mut seen: HashSet<i64> = HashSet::new();
    let mut deduped: i64 = 0;
    for row in group {
        match insert_payout_for_row(
            &mut *tx,
            cycle.id,
            tx_hash_bytes,
            earliest_ts,
            row,
            &mut seen,
            &mut deduped,
        )
        .await?
        {
            PayoutOutcome::Inserted(amount) => {
                stats.inserted += 1;
                cycle_total = cycle_total.saturating_add(amount);
                cycle_recipients = cycle_recipients.saturating_add(1);
            }
            PayoutOutcome::Skipped => stats.skipped += 1,
            PayoutOutcome::Rejected(reason) => {
                stats.rejected += 1;
                stats.rejected_amount = stats.rejected_amount.saturating_add(row.nacho_amount);
                warn!(id = row.id, reason, "nacho_payments row rejected");
            }
        }
    }
    stats.deduped_amount = deduped;

    payout::set_cycle_totals(&mut *tx, cycle.id, cycle_total, cycle_recipients).await?;
    payout::mark_cycle_broadcasting(&mut *tx, cycle.id).await?;
    payout::mark_cycle_settled(&mut *tx, cycle.id).await?;

    tx.commit().await?;
    Ok(stats)
}

enum PayoutOutcome {
    Inserted(i64),
    Skipped,
    Rejected(&'static str),
}

async fn insert_payout_for_row(
    tx: &mut sqlx::PgConnection,
    cycle_id: i64,
    tx_hash: BlockHash,
    earliest_ts: Option<NaiveDateTime>,
    row: &LegacyNachoPayment,
    seen: &mut HashSet<i64>,
    deduped: &mut i64,
) -> Result<PayoutOutcome, anyhow::Error> {
    if row.wallet_address.is_empty() {
        return Ok(PayoutOutcome::Rejected("wallet_address array empty"));
    }
    if row.wallet_address.len() > 1 {
        warn!(
            id = row.id,
            array_len = row.wallet_address.len(),
            "nacho_payments row has multi-entry wallet_address array — splitting amount evenly"
        );
    }

    let per_recipient = row.nacho_amount / (row.wallet_address.len() as i64);
    if per_recipient <= 0 {
        return Ok(PayoutOutcome::Rejected("per-recipient amount <= 0"));
    }

    let mut any_inserted = false;
    for addr in &row.wallet_address {
        let Ok(wallet_addr) = WalletAddress::new(addr.clone()) else {
            return Ok(PayoutOutcome::Rejected("wallet fails domain validation"));
        };
        let w = wallet::ensure(&mut *tx, &wallet_addr, LEGACY_NETWORK).await?;
        if !seen.insert(w.id.0) {
            // Same wallet already credited in this cycle this run — collapsed by
            // UNIQUE (cycle_id, wallet_id). Count it for the reconcile allowance.
            *deduped = deduped.saturating_add(per_recipient);
            continue;
        }
        // `false` = already present from a prior idempotent run (not a dedup).
        if insert_one_payout(tx, cycle_id, w.id, per_recipient, tx_hash, earliest_ts).await? {
            any_inserted = true;
        }
    }
    if any_inserted {
        Ok(PayoutOutcome::Inserted(row.nacho_amount))
    } else {
        Ok(PayoutOutcome::Skipped)
    }
}

async fn insert_one_payout(
    tx: &mut sqlx::PgConnection,
    cycle_id: i64,
    wallet_id: WalletId,
    amount: i64,
    tx_hash: BlockHash,
    earliest_ts: Option<NaiveDateTime>,
) -> Result<bool, anyhow::Error> {
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO payout
            (cycle_id, wallet_id, amount_sompi, status,
             krc20_commit_hash, krc20_reveal_hash,
             planned_at, submitted_at, confirmed_at)
         VALUES ($1, $2, $3, 'confirmed', $4, $4, $5, $5, $5)
         ON CONFLICT (cycle_id, wallet_id) DO NOTHING
         RETURNING id",
    )
    .bind(cycle_id)
    .bind(wallet_id.0)
    .bind(amount)
    .bind(tx_hash.as_bytes().to_vec())
    .bind(legacy_ts_to_utc(earliest_ts))
    .fetch_optional(&mut *tx)
    .await?;
    Ok(inserted.is_some())
}

fn classify_dry_run(row: &LegacyNachoPayment, mut stats: GroupStats) -> GroupStats {
    if row.wallet_address.is_empty() {
        stats.rejected += 1;
        stats.rejected_amount = stats.rejected_amount.saturating_add(row.nacho_amount);
        return stats;
    }
    let per = row.nacho_amount / (row.wallet_address.len() as i64);
    if per <= 0 {
        stats.rejected += 1;
        stats.rejected_amount = stats.rejected_amount.saturating_add(row.nacho_amount);
        return stats;
    }
    for addr in &row.wallet_address {
        if WalletAddress::new(addr.clone()).is_err() {
            stats.rejected += 1;
            stats.rejected_amount = stats.rejected_amount.saturating_add(row.nacho_amount);
            return stats;
        }
    }
    stats.inserted += 1;
    stats
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

fn parse_tx_hash(hex_str: &str) -> Option<BlockHash> {
    if hex_str.len() != 64 {
        return None;
    }
    BlockHash::from_hex(hex_str).ok()
}

fn legacy_ts_to_utc(ts: Option<NaiveDateTime>) -> DateTime<Utc> {
    ts.map_or_else(Utc::now, |t| t.and_utc())
}
