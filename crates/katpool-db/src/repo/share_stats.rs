//! Per-miner stats — read-side aggregations over the `share` and
//! `share_reject` tables.
//!
//! The HTTP API in Phase 6 composes these into the JSON the
//! frontend renders. The accountant uses some of them in its
//! Prometheus exporter so a Grafana dashboard can chart per-miner
//! hashrate without scraping every miner directly.
//!
//! All functions are **read-only** — they never write to the
//! database. Time-range queries take an inclusive `since`
//! `DateTime<Utc>`.

// `float_arithmetic` is denied workspace-wide because most of our
// money math must be integer. Hashrate estimates are the
// exception: they're floating-point by definition (H/s is a rate,
// not a sompi figure), and Phase 6 surfaces them as JSON numbers
// where lossy f64 is the accepted representation.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::float_arithmetic
)]

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;

use crate::DbError;
use crate::repo::WalletId;

/// Per-miner accepted-share aggregates over a time window.
#[derive(Debug, Clone, Copy, PartialEq, sqlx::FromRow)]
pub struct AcceptedShareStats {
    /// Count of accepted shares.
    pub share_count: i64,
    /// Sum of share difficulty — the PROP weight contribution.
    pub total_weight: f64,
}

/// Accepted share aggregate for one wallet since `since`.
///
/// Returns `(0, 0.0)` for wallets with no shares in the window
/// — that's the legitimate "no activity" answer, not a
/// not-found case.
pub async fn accepted_for_wallet<'e, E>(
    executor: E,
    wallet_id: WalletId,
    since: DateTime<Utc>,
) -> Result<AcceptedShareStats, DbError>
where
    E: PgExecutor<'e>,
{
    let row: (Option<i64>, Option<f64>) = sqlx::query_as(
        "SELECT count(*)::bigint, sum(difficulty)
           FROM share
          WHERE wallet_id = $1
            AND credited_at >= $2",
    )
    .bind(wallet_id.0)
    .bind(since)
    .fetch_one(executor)
    .await?;
    Ok(AcceptedShareStats {
        share_count: row.0.unwrap_or(0),
        total_weight: row.1.unwrap_or(0.0),
    })
}

/// Pool-wide accepted share aggregate since `since`.
pub async fn accepted_pool_wide<'e, E>(
    executor: E,
    since: DateTime<Utc>,
) -> Result<AcceptedShareStats, DbError>
where
    E: PgExecutor<'e>,
{
    let row: (Option<i64>, Option<f64>) = sqlx::query_as(
        "SELECT count(*)::bigint, sum(difficulty)
           FROM share
          WHERE credited_at >= $1",
    )
    .bind(since)
    .fetch_one(executor)
    .await?;
    Ok(AcceptedShareStats {
        share_count: row.0.unwrap_or(0),
        total_weight: row.1.unwrap_or(0.0),
    })
}

/// Estimated hashrate over a sliding wall-clock window, in H/s.
///
/// Computation: `sum(difficulty * 2^32) / window_secs`. The
/// factor of 2^32 comes from the share-difficulty convention —
/// one share of difficulty D represents D × 2^32 expected hashes.
///
/// Returns 0.0 if the window has zero shares.
pub async fn hashrate_estimate_for_wallet<'e, E>(
    executor: E,
    wallet_id: WalletId,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<f64, DbError>
where
    E: PgExecutor<'e>,
{
    if until <= since {
        return Err(DbError::Config {
            message: "hashrate_estimate_for_wallet: until must be after since".to_owned(),
        });
    }
    let weight: Option<f64> = sqlx::query_scalar(
        "SELECT sum(difficulty)
           FROM share
          WHERE wallet_id = $1
            AND credited_at >= $2
            AND credited_at <  $3",
    )
    .bind(wallet_id.0)
    .bind(since)
    .bind(until)
    .fetch_one(executor)
    .await?;
    let weight = weight.unwrap_or(0.0);
    let secs = (until - since).num_seconds().max(1) as f64;
    // 2^32 is exact in f64 (only 33 significant bits).
    Ok(weight * 4_294_967_296.0 / secs)
}

/// Pool-wide estimated hashrate over the same window.
pub async fn hashrate_estimate_pool_wide<'e, E>(
    executor: E,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<f64, DbError>
where
    E: PgExecutor<'e>,
{
    if until <= since {
        return Err(DbError::Config {
            message: "hashrate_estimate_pool_wide: until must be after since".to_owned(),
        });
    }
    let weight: Option<f64> = sqlx::query_scalar(
        "SELECT sum(difficulty)
           FROM share
          WHERE credited_at >= $1
            AND credited_at <  $2",
    )
    .bind(since)
    .bind(until)
    .fetch_one(executor)
    .await?;
    let weight = weight.unwrap_or(0.0);
    let secs = (until - since).num_seconds().max(1) as f64;
    Ok(weight * 4_294_967_296.0 / secs)
}

/// Combined accepted + rejected counts for a wallet — both since
/// `since` so the caller sees a consistent time window.
///
/// One round-trip; the SQL emits a single row with both halves.
pub async fn accepted_and_rejected_for_wallet<'e, E>(
    executor: E,
    wallet_id: WalletId,
    since: DateTime<Utc>,
) -> Result<WalletShareSummary, DbError>
where
    E: PgExecutor<'e>,
{
    let row: (Option<i64>, Option<f64>, Option<i64>) = sqlx::query_as(
        "SELECT
           (SELECT count(*)::bigint
              FROM share
             WHERE wallet_id = $1 AND credited_at >= $2),
           (SELECT sum(difficulty)
              FROM share
             WHERE wallet_id = $1 AND credited_at >= $2),
           (SELECT count(*)::bigint
              FROM share_reject
             WHERE wallet_id = $1 AND rejected_at >= $2)",
    )
    .bind(wallet_id.0)
    .bind(since)
    .fetch_one(executor)
    .await?;
    Ok(WalletShareSummary {
        accepted_count: row.0.unwrap_or(0),
        accepted_weight: row.1.unwrap_or(0.0),
        rejected_count: row.2.unwrap_or(0),
    })
}

/// One-shot summary of a wallet's share activity. Used by the
/// Phase 6 API's `/miner/{address}` JSON.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WalletShareSummary {
    /// Accepted shares since the window start.
    pub accepted_count: i64,
    /// Sum of accepted share difficulty.
    pub accepted_weight: f64,
    /// Rejected shares since the window start (across all reasons).
    pub rejected_count: i64,
}
