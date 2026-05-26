//! `payments` → `payout_cycle` (kind=kas) + `payout` transform.
//!
//! ## Grouping
//!
//! Multiple legacy `payments` rows can share the same
//! `transaction_hash`: that's the batch-payout case where a single
//! on-chain transaction settled multiple recipients in one tx.
//! Each unique `transaction_hash` becomes one `payout_cycle` in
//! the new schema; each row under that hash becomes one `payout`.
//!
//! ## DAA range
//!
//! The legacy schema doesn't record the DAA range a cycle covered.
//! The new schema's `payout_cycle.daa_start`/`daa_end` are required
//! and `CHECK`-enforced (`end > start`). We use synthetic ranges:
//! `(daa_start = 0, daa_end = 1)` for every imported cycle. The
//! `idempotency_key` is human-readable (`kas-legacy-<tx_hash>`)
//! and serves as the actual cycle identity — operators identify
//! legacy-imported cycles by the `legacy-` infix.
//!
//! ## Wallet-address arrays
//!
//! `payments.wallet_address` is `text[]` in the legacy schema. In
//! the production data every row's array contains exactly one
//! address (see `docs/db-schema.md`'s Legacy reference table), but
//! the importer fans out to N payouts if the array has > 1 entry
//! for defensive parity.

// Allowed lints: monetary amounts are integer sompi (1 sompi is
// the indivisible smallest unit), so integer_division is the
// correct operator. cast_possible_wrap is safe because batch
// sizes never approach u32::MAX. cognitive_complexity fires on
// straight-line aggregate-counting code that's hard to factor.
#![allow(
    clippy::cast_possible_wrap,
    clippy::integer_division,
    clippy::cognitive_complexity,
    clippy::explicit_auto_deref
)]

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use katpool_db::repo::payout::{self, PayoutKind};
use katpool_db::repo::{WalletId, wallet};
use katpool_domain::{BlockHash, DaaScore, WalletAddress};
use tracing::{debug, info, warn};

use crate::source::{self, LegacyPayment};
use crate::transform::TransformStats;

const LEGACY_NETWORK: &str = "mainnet";

/// Run the `payments` → `payout_cycle`/`payout` transform.
pub async fn run(
    source: &sqlx::PgPool,
    target: &sqlx::PgPool,
    dry_run: bool,
) -> Result<TransformStats, anyhow::Error> {
    let rows = source::fetch_payments(source).await?;
    info!(row_count = rows.len(), dry_run, "starting payments import");

    let mut stats = TransformStats::default();

    // Group rows by transaction_hash. BTreeMap to keep deterministic
    // ordering across runs (helps reproducible audit-log forensics).
    let mut groups: BTreeMap<String, Vec<LegacyPayment>> = BTreeMap::new();
    for row in rows {
        groups
            .entry(row.transaction_hash.clone())
            .or_default()
            .push(row);
    }
    info!(unique_cycles = groups.len(), "payments grouped into cycles");

    for (tx_hash, group) in groups {
        stats.read += group.len() as u64;
        match import_group(target, &tx_hash, &group, dry_run).await {
            Ok(g) => {
                stats.inserted += g.inserted;
                stats.skipped += g.skipped;
                stats.rejected += g.rejected;
            }
            Err(e) => return Err(e.context(format!("import payments cycle tx_hash={tx_hash}"))),
        }
    }

    info!(stats = %stats, "payments import complete");
    Ok(stats)
}

#[derive(Debug, Default, Clone, Copy)]
struct GroupStats {
    inserted: u64,
    skipped: u64,
    rejected: u64,
}

async fn import_group(
    target: &sqlx::PgPool,
    tx_hash: &str,
    group: &[LegacyPayment],
    dry_run: bool,
) -> Result<GroupStats, anyhow::Error> {
    let mut stats = GroupStats::default();
    let Some(tx_hash_bytes) = parse_tx_hash(tx_hash) else {
        // The on-chain tx hash itself didn't parse — reject every
        // row in this cycle.
        stats.rejected += group.len() as u64;
        warn!(tx_hash, "payments cycle rejected: tx_hash not 64-char hex");
        return Ok(stats);
    };

    let key = format!("kas-legacy-{tx_hash}");

    if dry_run {
        // Dry-run: classify each row as inserted-or-rejected without
        // touching the target.
        for row in group {
            stats = classify_dry_run(row, stats);
        }
        return Ok(stats);
    }

    let mut tx = target.begin().await?;

    let cycle = create_legacy_cycle(&mut *tx, PayoutKind::Kas, &key).await?;

    let mut cycle_total: i64 = 0;
    let mut cycle_recipients: i32 = 0;
    let earliest_ts = group_earliest_ts(group);
    for row in group {
        match insert_payout_for_row(&mut *tx, cycle.id, tx_hash_bytes, earliest_ts, row).await? {
            PayoutOutcome::Inserted(amount) => {
                stats.inserted += 1;
                cycle_total = cycle_total.saturating_add(amount);
                cycle_recipients = cycle_recipients.saturating_add(1);
            }
            PayoutOutcome::Skipped => stats.skipped += 1,
            PayoutOutcome::Rejected(reason) => {
                stats.rejected += 1;
                warn!(id = row.id, reason, "payments row rejected");
            }
        }
    }

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
    row: &LegacyPayment,
) -> Result<PayoutOutcome, anyhow::Error> {
    // Legacy `wallet_address` is `text[]`. Production data always has
    // exactly one entry per row, but the importer is defensive and
    // splits multi-entry rows by amount-per-recipient ÷ N. We log a
    // warning when the array has length != 1 so we know if any
    // production data violates the single-entry assumption.
    if row.wallet_address.is_empty() {
        return Ok(PayoutOutcome::Rejected("wallet_address array empty"));
    }
    if row.wallet_address.len() > 1 {
        warn!(
            id = row.id,
            array_len = row.wallet_address.len(),
            "payments row has multi-entry wallet_address array — splitting amount evenly"
        );
    }

    let per_recipient_sompi = row.amount / (row.wallet_address.len() as i64);
    if per_recipient_sompi <= 0 {
        return Ok(PayoutOutcome::Rejected("per-recipient amount <= 0"));
    }

    // Resolve each recipient address → wallet row.
    for addr in &row.wallet_address {
        let Ok(wallet_addr) = WalletAddress::new(addr.clone()) else {
            return Ok(PayoutOutcome::Rejected("wallet fails domain validation"));
        };
        let w = wallet::ensure(&mut *tx, &wallet_addr, LEGACY_NETWORK).await?;
        let outcome = insert_one_payout(
            tx,
            cycle_id,
            w.id,
            per_recipient_sompi,
            tx_hash,
            earliest_ts,
        )
        .await?;
        if matches!(outcome, InsertOutcome::AlreadyExists) {
            debug!(cycle_id, wallet = addr, "payout already exists; skip");
            return Ok(PayoutOutcome::Skipped);
        }
    }
    Ok(PayoutOutcome::Inserted(row.amount))
}

enum InsertOutcome {
    Inserted,
    AlreadyExists,
}

async fn insert_one_payout(
    tx: &mut sqlx::PgConnection,
    cycle_id: i64,
    wallet_id: WalletId,
    amount_sompi: i64,
    tx_hash: BlockHash,
    earliest_ts: Option<NaiveDateTime>,
) -> Result<InsertOutcome, anyhow::Error> {
    // Manual INSERT (not `payout::insert_payout`) so we can use
    // ON CONFLICT DO NOTHING for idempotency. The cycle's
    // UNIQUE (cycle_id, wallet_id) constraint is what guards.
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO payout
            (cycle_id, wallet_id, amount_sompi, status, tx_hash,
             planned_at, submitted_at, confirmed_at)
         VALUES ($1, $2, $3, 'confirmed', $4, $5, $5, $5)
         ON CONFLICT (cycle_id, wallet_id) DO NOTHING
         RETURNING id",
    )
    .bind(cycle_id)
    .bind(wallet_id.0)
    .bind(amount_sompi)
    .bind(tx_hash.as_bytes().to_vec())
    .bind(legacy_ts_to_utc(earliest_ts))
    .fetch_optional(&mut *tx)
    .await?;
    Ok(if inserted.is_some() {
        InsertOutcome::Inserted
    } else {
        InsertOutcome::AlreadyExists
    })
}

fn classify_dry_run(row: &LegacyPayment, mut stats: GroupStats) -> GroupStats {
    if row.wallet_address.is_empty() {
        stats.rejected += 1;
        return stats;
    }
    let per = row.amount / (row.wallet_address.len() as i64);
    if per <= 0 {
        stats.rejected += 1;
        return stats;
    }
    for addr in &row.wallet_address {
        if WalletAddress::new(addr.clone()).is_err() {
            stats.rejected += 1;
            return stats;
        }
    }
    stats.inserted += 1;
    stats
}

/// Find the earliest timestamp across a group of rows — used as the
/// cycle's `planned_at`/`broadcast_at`/`settled_at` since the legacy
/// schema doesn't distinguish those.
fn group_earliest_ts(group: &[LegacyPayment]) -> Option<NaiveDateTime> {
    group.iter().filter_map(|r| r.timestamp).min()
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

/// Re-export `DaaScore` for symmetry with the rest of the transform
/// modules — currently unused here but kept so cross-module
/// signatures stay parameterizable on the (kind, daa range) pair
/// when the importer evolves.
#[allow(dead_code)]
const _LEGACY_DAA_RANGE: (DaaScore, DaaScore) = (DaaScore::new(0), DaaScore::new(1));
