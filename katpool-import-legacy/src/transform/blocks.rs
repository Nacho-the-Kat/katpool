//! `block_details` → (`wallet`, `worker`, `block`) transform.
//!
//! Drives the entity-identity creation for the new schema: every
//! distinct miner wallet seen in `block_details` becomes a `wallet`
//! row, every `(wallet, miner_id)` pair becomes a `worker` row, and
//! every legacy block row becomes a `block` row in the `matured`
//! lifecycle state (the legacy data only retains blocks that paid
//! out — orphans were dropped).
//!
//! Idempotent: re-running re-hits all three repo `ensure`/`insert`
//! paths with their `ON CONFLICT` semantics. The schema's
//! `block.hash` UNIQUE constraint distinguishes "already imported"
//! from "first run".

#![allow(
    clippy::cast_possible_wrap,
    // Progress reporting only — float precision is irrelevant for a
    // "X / Y (Z%)" log line.
    clippy::cast_precision_loss,
    clippy::float_arithmetic,
)]

use chrono::{DateTime, NaiveDateTime, Utc};
use katpool_db::repo::{wallet, worker};
use katpool_domain::{BlockHash, CorrelationId, DaaScore, WalletAddress, WorkerName};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::source::{self, LegacyBlockDetail};
use crate::transform::TransformStats;

/// Configurable batch size for the page-walk through `block_details`.
/// 5000 keeps each transaction short enough to avoid lock-table
/// pressure while amortising network round-trips on the source side.
const PAGE_SIZE: i64 = 5_000;

/// Network the legacy pool ran on. Legacy was mainnet-only; the
/// importer hard-codes this rather than scraping it from row data.
const LEGACY_NETWORK: &str = "mainnet";

/// Run the `block_details` → block-aggregate import.
///
/// `dry_run` short-circuits the actual writes — useful for getting a
/// reconciliation count without touching the target.
pub async fn run(
    source: &sqlx::PgPool,
    target: &sqlx::PgPool,
    dry_run: bool,
) -> Result<TransformStats, anyhow::Error> {
    let total = source::count_block_details(source).await?;
    info!(total_rows = total, dry_run, "starting block_details import");

    let mut stats = TransformStats::default();
    let mut last_ts: Option<NaiveDateTime> = None;
    let mut last_hash: Option<String> = None;

    loop {
        let page =
            source::page_block_details(source, last_ts, last_hash.as_deref(), PAGE_SIZE).await?;
        if page.is_empty() {
            break;
        }
        let page_len = page.len();

        for row in &page {
            stats.read += 1;
            process_one(target, row, dry_run, &mut stats).await?;
        }

        // Advance cursor.
        if let Some(last) = page.last() {
            last_ts = last.timestamp;
            last_hash = Some(last.mined_block_hash.clone());
        }

        log_progress(stats.read, total);

        if (page_len as i64) < PAGE_SIZE {
            break;
        }
    }

    info!(stats = %stats, "block_details import complete");
    Ok(stats)
}

/// Per-row import with classification + counter bump. Hard errors
/// (connection lost) bubble up; soft errors (validation failures)
/// just bump the `rejected` counter and log.
async fn process_one(
    target: &sqlx::PgPool,
    row: &LegacyBlockDetail,
    dry_run: bool,
    stats: &mut TransformStats,
) -> Result<(), anyhow::Error> {
    match import_one(target, row, dry_run).await {
        Ok(Imported::Inserted) => stats.inserted += 1,
        Ok(Imported::Skipped) => stats.skipped += 1,
        Ok(Imported::Rejected(reason)) => {
            stats.rejected += 1;
            warn!(hash = %row.mined_block_hash, reason, "block row rejected");
        }
        Err(e) => return Err(e.context(format!("import block {}", row.mined_block_hash))),
    }
    Ok(())
}

/// One log line every 50k rows.
fn log_progress(read: u64, total: i64) {
    if !read.is_multiple_of(50_000) {
        return;
    }
    let pct = (read as f64) * 100.0 / total.max(1) as f64;
    info!(
        progress = format!("{read}/{total} ({pct:.1}%)"),
        "block_details progress"
    );
}

#[derive(Debug)]
enum Imported {
    Inserted,
    Skipped,
    Rejected(&'static str),
}

/// Validated form of a legacy row, ready for persistence.
struct Parsed {
    wallet_addr: WalletAddress,
    worker_name: WorkerName,
    block_hash: BlockHash,
    daa: DaaScore,
    miner_reward: i64,
    timestamp: DateTime<Utc>,
    /// Raw hex form, kept for stable `correlation_id` derivation and
    /// for log lines that want the operator-readable identifier.
    raw_hash_hex: String,
}

/// Pure validation: legacy row → typed `Parsed` or a reject reason.
/// Pulled out of `import_one` to keep that function's cognitive
/// complexity reasonable and to make validation rules independently
/// testable.
fn parse_legacy_row(row: &LegacyBlockDetail) -> Result<Parsed, &'static str> {
    let Some(wallet_str) = row.wallet.as_ref() else {
        return Err("missing wallet");
    };
    let Ok(wallet_addr) = WalletAddress::new(wallet_str.clone()) else {
        return Err("wallet fails domain validation");
    };
    let Some(worker_str) = row.miner_id.as_ref().filter(|s| !s.is_empty()) else {
        return Err("missing miner_id (worker)");
    };
    let Ok(worker_name) = WorkerName::new(worker_str.clone()) else {
        return Err("worker name fails domain validation");
    };
    let Some(block_hash) = parse_block_hash(&row.mined_block_hash) else {
        return Err("mined_block_hash not 64-char hex");
    };
    let Some(daa) = row
        .daa_score
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok())
        .map(DaaScore::new)
    else {
        return Err("daa_score not parseable as u64");
    };
    if row.miner_reward < 0 {
        return Err("miner_reward negative");
    }
    Ok(Parsed {
        wallet_addr,
        worker_name,
        block_hash,
        daa,
        miner_reward: row.miner_reward,
        timestamp: legacy_ts_to_utc(row.timestamp),
        raw_hash_hex: row.mined_block_hash.clone(),
    })
}

async fn import_one(
    target: &sqlx::PgPool,
    row: &LegacyBlockDetail,
    dry_run: bool,
) -> Result<Imported, anyhow::Error> {
    let parsed = match parse_legacy_row(row) {
        Ok(p) => p,
        Err(reason) => return Ok(Imported::Rejected(reason)),
    };

    if dry_run {
        return Ok(Imported::Inserted);
    }

    let mut tx = target.begin().await?;
    let w = wallet::ensure(&mut *tx, &parsed.wallet_addr, LEGACY_NETWORK).await?;
    let wk = worker::ensure(&mut *tx, w.id, &parsed.worker_name).await?;

    // Synthesize a stable correlation id from the block hash so re-
    // running the importer produces the same correlation id for the
    // same block, which makes audit-log forensics deterministic.
    let correlation_id = synth_correlation_id(&parsed.raw_hash_hex);

    // Manual INSERT (not `block::insert`) because we need the
    // `ON CONFLICT DO NOTHING` semantics for idempotency on re-
    // import. `block::insert` returns `DbError::Constraint` on a
    // duplicate hash; we want a silent skip.
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO block
            (hash, finder_wallet_id, finder_worker_id, daa_score, nonce,
             correlation_id, status, found_at, submitted_at, confirmed_at,
             matured_at, miner_reward_sompi)
         VALUES ($1, $2, $3, $4, $5, $6, 'matured', $7, $7, $7, $7, $8)
         ON CONFLICT (hash) DO NOTHING
         RETURNING id",
    )
    .bind(parsed.block_hash.as_bytes().to_vec())
    .bind(w.id.0)
    .bind(wk.id.0)
    .bind(parsed.daa.value() as i64)
    .bind(0_i64) // legacy schema doesn't store nonce; fill with 0
    .bind(correlation_id.as_uuid())
    .bind(parsed.timestamp)
    .bind(parsed.miner_reward)
    .fetch_optional(&mut *tx)
    .await?;

    tx.commit().await?;

    if inserted.is_some() {
        debug!(hash = %parsed.raw_hash_hex, "block imported");
        Ok(Imported::Inserted)
    } else {
        Ok(Imported::Skipped)
    }
}

/// Convert the legacy 64-char-hex string to a [`BlockHash`].
fn parse_block_hash(hex_str: &str) -> Option<BlockHash> {
    if hex_str.len() != 64 {
        return None;
    }
    BlockHash::from_hex(hex_str).ok()
}

/// Deterministically derive a v5 UUID from the block hash so the
/// `correlation_id` column is reproducible across re-imports. Uses
/// the DNS namespace (the choice is arbitrary; consistency is what
/// matters).
fn synth_correlation_id(block_hash_hex: &str) -> CorrelationId {
    let uuid = Uuid::new_v5(&Uuid::NAMESPACE_DNS, block_hash_hex.as_bytes());
    CorrelationId::from_uuid(uuid)
}

/// Best-effort interpretation of legacy naive timestamps as UTC. The
/// legacy schema uses `timestamp without time zone`; based on the
/// production deployment we know these are UTC, so we attach `Utc`
/// directly. If the source ever ships non-UTC naive timestamps we
/// will see them as off-by-an-hour-or-two — acceptable for an
/// audit-trail field; the new schema's `*_at` columns are
/// timestamptz so going forward the issue is moot.
fn legacy_ts_to_utc(ts: Option<NaiveDateTime>) -> DateTime<Utc> {
    ts.map_or_else(Utc::now, |t| t.and_utc())
}
