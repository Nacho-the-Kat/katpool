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
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::float_arithmetic
)]

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;

use crate::DbError;
use crate::repo::{WalletId, WorkerId};

/// The share-difficulty → expected-hashes constant. A share of
/// difficulty `D` represents `D × 2^32` expected hashes; dividing the
/// summed difficulty by the window's wall-clock seconds yields H/s.
/// `2^32` is exact in `f64` (only 33 significant bits).
const HASHES_PER_DIFFICULTY: f64 = 4_294_967_296.0;

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
    Ok(weight * HASHES_PER_DIFFICULTY / secs)
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
    Ok(weight * HASHES_PER_DIFFICULTY / secs)
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

/// Accepted share aggregate for one worker since `since`.
///
/// Returns `(0, 0.0)` for workers with no shares in the window — the
/// legitimate "no activity" answer, not a not-found case. Drives the
/// Phase 6 API's per-worker (`/miners/{address}/workers`) breakdown.
pub async fn accepted_for_worker<'e, E>(
    executor: E,
    worker_id: WorkerId,
    since: DateTime<Utc>,
) -> Result<AcceptedShareStats, DbError>
where
    E: PgExecutor<'e>,
{
    let row: (Option<i64>, Option<f64>) = sqlx::query_as(
        "SELECT count(*)::bigint, sum(difficulty)
           FROM share
          WHERE worker_id = $1
            AND credited_at >= $2",
    )
    .bind(worker_id.0)
    .bind(since)
    .fetch_one(executor)
    .await?;
    Ok(AcceptedShareStats {
        share_count: row.0.unwrap_or(0),
        total_weight: row.1.unwrap_or(0.0),
    })
}

/// Estimated hashrate for one worker over a sliding window, in H/s.
///
/// Same `sum(difficulty) × 2^32 / window_secs` convention as
/// [`hashrate_estimate_for_wallet`]. Returns 0.0 for an empty window.
pub async fn hashrate_estimate_for_worker<'e, E>(
    executor: E,
    worker_id: WorkerId,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<f64, DbError>
where
    E: PgExecutor<'e>,
{
    if until <= since {
        return Err(DbError::Config {
            message: "hashrate_estimate_for_worker: until must be after since".to_owned(),
        });
    }
    let weight: Option<f64> = sqlx::query_scalar(
        "SELECT sum(difficulty)
           FROM share
          WHERE worker_id = $1
            AND credited_at >= $2
            AND credited_at <  $3",
    )
    .bind(worker_id.0)
    .bind(since)
    .bind(until)
    .fetch_one(executor)
    .await?;
    let weight = weight.unwrap_or(0.0);
    let secs = (until - since).num_seconds().max(1) as f64;
    Ok(weight * HASHES_PER_DIFFICULTY / secs)
}

/// Distinct active wallets and workers (≥ 1 accepted share) since
/// `since`. One round-trip; drives the pool-stats "miners online"
/// figure.
pub async fn active_participant_counts<'e, E>(
    executor: E,
    since: DateTime<Utc>,
) -> Result<ActiveCounts, DbError>
where
    E: PgExecutor<'e>,
{
    let row: (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT count(DISTINCT wallet_id)::bigint,
                count(DISTINCT worker_id)::bigint
           FROM share
          WHERE credited_at >= $1",
    )
    .bind(since)
    .fetch_one(executor)
    .await?;
    Ok(ActiveCounts {
        wallets: row.0.unwrap_or(0),
        workers: row.1.unwrap_or(0),
    })
}

/// Distinct-participant counts over a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveCounts {
    /// Distinct wallets with ≥ 1 accepted share.
    pub wallets: i64,
    /// Distinct workers with ≥ 1 accepted share.
    pub workers: i64,
}

/// One point of a hashrate time-series: the bucket's start instant and
/// the estimated hashrate (H/s) over that bucket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HashratePoint {
    /// Inclusive bucket start (UTC), aligned to a `bucket_secs` grid.
    pub bucket_start: DateTime<Utc>,
    /// Estimated hashrate over the bucket in H/s.
    pub hashrate: f64,
}

/// Validate the shared arguments of the series queries.
fn validate_series_args(
    from: DateTime<Utc>,
    until: DateTime<Utc>,
    bucket_secs: i64,
    what: &str,
) -> Result<f64, DbError> {
    if until <= from {
        return Err(DbError::Config {
            message: format!("{what}: until must be after from"),
        });
    }
    if bucket_secs <= 0 {
        return Err(DbError::Config {
            message: format!("{what}: bucket_secs must be positive, got {bucket_secs}"),
        });
    }
    Ok(bucket_secs as f64)
}

/// Build [`HashratePoint`]s from raw `(bucket_epoch, weight)` rows.
///
/// `bucket_epoch` is the integer bucket-grid second; the weight is the
/// summed share difficulty in that bucket. Hashrate is
/// `weight × 2^32 / bucket_secs`.
fn points_from_rows(
    rows: Vec<(f64, Option<f64>)>,
    bucket_secs_f: f64,
) -> Result<Vec<HashratePoint>, DbError> {
    rows.into_iter()
        .map(|(epoch, weight)| {
            let secs = epoch.round();
            // `chrono` from a non-negative epoch second; share timestamps are
            // post-2024 so the cast and conversion never go negative.
            let bucket_start =
                DateTime::<Utc>::from_timestamp(secs as i64, 0).ok_or_else(|| DbError::Config {
                    message: format!("hashrate series: bucket epoch {secs} out of range"),
                })?;
            Ok(HashratePoint {
                bucket_start,
                hashrate: weight.unwrap_or(0.0) * HASHES_PER_DIFFICULTY / bucket_secs_f,
            })
        })
        .collect()
}

/// Pool-wide hashrate time-series over `[from, until)`, bucketed to a
/// `bucket_secs`-second grid (aligned to the unix epoch). Empty buckets
/// are omitted; the caller zero-fills if it needs a dense series.
///
/// The caller (Phase 6 API) is responsible for bounding the span and
/// bucket count before calling; this function only rejects a
/// non-positive bucket or an empty/inverted range.
pub async fn hashrate_series_pool_wide<'e, E>(
    executor: E,
    from: DateTime<Utc>,
    until: DateTime<Utc>,
    bucket_secs: i64,
) -> Result<Vec<HashratePoint>, DbError>
where
    E: PgExecutor<'e>,
{
    let bucket_secs_f =
        validate_series_args(from, until, bucket_secs, "hashrate_series_pool_wide")?;
    let rows: Vec<(f64, Option<f64>)> = sqlx::query_as(
        "SELECT floor(extract(epoch FROM credited_at) / $3::double precision)
                    * $3::double precision AS bucket_epoch,
                sum(difficulty) AS weight
           FROM share
          WHERE credited_at >= $1
            AND credited_at <  $2
          GROUP BY bucket_epoch
          ORDER BY bucket_epoch ASC",
    )
    .bind(from)
    .bind(until)
    .bind(bucket_secs)
    .fetch_all(executor)
    .await?;
    points_from_rows(rows, bucket_secs_f)
}

/// Per-wallet hashrate time-series over `[from, until)`, same bucketing
/// as [`hashrate_series_pool_wide`].
pub async fn hashrate_series_for_wallet<'e, E>(
    executor: E,
    wallet_id: WalletId,
    from: DateTime<Utc>,
    until: DateTime<Utc>,
    bucket_secs: i64,
) -> Result<Vec<HashratePoint>, DbError>
where
    E: PgExecutor<'e>,
{
    let bucket_secs_f =
        validate_series_args(from, until, bucket_secs, "hashrate_series_for_wallet")?;
    let rows: Vec<(f64, Option<f64>)> = sqlx::query_as(
        "SELECT floor(extract(epoch FROM credited_at) / $4::double precision)
                    * $4::double precision AS bucket_epoch,
                sum(difficulty) AS weight
           FROM share
          WHERE wallet_id = $1
            AND credited_at >= $2
            AND credited_at <  $3
          GROUP BY bucket_epoch
          ORDER BY bucket_epoch ASC",
    )
    .bind(wallet_id.0)
    .bind(from)
    .bind(until)
    .bind(bucket_secs)
    .fetch_all(executor)
    .await?;
    points_from_rows(rows, bucket_secs_f)
}
