//! Pool-wide aggregate handlers, served through the pool TTL cache.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use serde_json::Value;

use katpool_db::repo::{block, connection_session, payout, share_reject, share_stats, treasury};

use crate::error::ApiError;
use crate::handlers::{cached_json, resolve_window, to_value};
use crate::models::{
    ActiveMinersHistory, ActiveMinersPointView, BlockCounts, BlockView, BlocksPage, CycleView,
    CyclesPage, FirmwareBreakdown, FirmwareEntryView, GeoBreakdown, GeoEntryView, HashrateHistory,
    HashratePointView, HashrateSnapshot, LeaderboardEntryView, LeaderboardResponse, PayoutTotals,
    PoolRejectsResponse, PoolStats, RejectReasonCount, TreasuryView,
};
use crate::money::KasAmount;
use crate::params::{self, LeaderboardParams, PageParams, RangeParams, WindowParams};
use crate::state::AppState;

/// `GET /api/v1/pool/stats` — headline pool figures over a sliding window.
pub async fn stats(
    State(state): State<AppState>,
    Query(window_params): Query<WindowParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let window = params::window(&window_params)?;
    let key = format!("pool/stats?w={}", window.as_secs());
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, build_stats(state, window)).await
}

async fn build_stats(state: AppState, window: std::time::Duration) -> Result<Value, ApiError> {
    let w = resolve_window(window);

    let accepted = share_stats::accepted_pool_wide(&state.pool, w.since).await?;
    let hashrate_hs =
        share_stats::hashrate_estimate_pool_wide(&state.pool, w.since, w.until).await?;
    let counts = share_stats::active_participant_counts(&state.pool, w.since).await?;
    let block_rows = block::count_by_status(&state.pool).await?;
    let totals = payout::pool_payout_totals(&state.pool).await?;
    let treasury_snapshot = treasury::latest(&state.pool).await?;

    let resp = PoolStats {
        window_secs: w.secs,
        miners_active: counts.wallets,
        workers_active: counts.workers,
        hashrate_hs,
        accepted_shares: accepted.share_count,
        blocks: BlockCounts::from_rows(&block_rows),
        payouts: PayoutTotals {
            kas_confirmed: KasAmount::from_sompi(totals.kas_confirmed_sompi),
            nacho_confirmed: KasAmount::from_sompi(totals.nacho_confirmed_sompi),
            confirmed_payouts: totals.confirmed_payouts,
        },
        treasury: treasury_snapshot.map(|t| TreasuryView {
            captured_at: t.captured_at,
            kas_balance: KasAmount::from_sompi(t.kas_balance_sompi),
            nacho_balance: t.nacho_balance.to_string(),
            daa_score: t.daa_score,
            blue_score: t.blue_score,
        }),
    };
    to_value(&resp)
}

/// `GET /api/v1/pool/hashrate` — current pool hashrate estimate.
pub async fn hashrate(
    State(state): State<AppState>,
    Query(window_params): Query<WindowParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let window = params::window(&window_params)?;
    let key = format!("pool/hashrate?w={}", window.as_secs());
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let w = resolve_window(window);
        let hashrate_hs =
            share_stats::hashrate_estimate_pool_wide(&state.pool, w.since, w.until).await?;
        to_value(&HashrateSnapshot {
            hashrate_hs,
            window_secs: w.secs,
        })
    })
    .await
}

/// `GET /api/v1/pool/hashrate/history` — bucketed pool hashrate series.
pub async fn hashrate_history(
    State(state): State<AppState>,
    Query(range_params): Query<RangeParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let range = params::range(&range_params)?;
    let key = format!(
        "pool/hashrate/history?from={}&to={}&b={}",
        range.from.timestamp(),
        range.until.timestamp(),
        range.bucket.seconds()
    );
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let points = share_stats::hashrate_series_pool_wide(
            &state.pool,
            range.from,
            range.until,
            range.bucket.seconds(),
        )
        .await?;
        to_value(&HashrateHistory {
            from: range.from,
            to: range.until,
            bucket: bucket_token(range.bucket),
            points: points
                .into_iter()
                .map(|p| HashratePointView {
                    bucket_start: p.bucket_start,
                    hashrate_hs: p.hashrate,
                })
                .collect(),
        })
    })
    .await
}

/// `GET /api/v1/pool/blocks` — recent blocks, keyset-paginated.
pub async fn blocks(
    State(state): State<AppState>,
    Query(page_params): Query<PageParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let page = params::page(&page_params)?;
    let key = format!("pool/blocks?l={}&before={:?}", page.limit, page.before_id);
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let rows = block::list_recent(&state.pool, page.limit, page.before_id).await?;
        let next_before = next_cursor(rows.len(), page.limit, rows.last().map(|b| b.id.0));
        let blocks = rows.iter().map(BlockView::from).collect();
        to_value(&BlocksPage {
            blocks,
            next_before,
        })
    })
    .await
}

/// `GET /api/v1/pool/payouts` — recent payout cycles, keyset-paginated.
pub async fn payouts(
    State(state): State<AppState>,
    Query(page_params): Query<PageParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let page = params::page(&page_params)?;
    let key = format!("pool/payouts?l={}&before={:?}", page.limit, page.before_id);
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let rows = payout::list_recent_cycles(&state.pool, page.limit, page.before_id).await?;
        let next_before = next_cursor(rows.len(), page.limit, rows.last().map(|c| c.id));
        let cycles = rows.iter().map(CycleView::from).collect();
        to_value(&CyclesPage {
            cycles,
            next_before,
        })
    })
    .await
}

/// `GET /api/v1/pool/leaderboard` — top miners by window hashrate.
// `pool_share` is a unitless ratio of summed difficulty (a rate-like
// figure), not money; float division is the correct representation here.
#[allow(clippy::float_arithmetic)]
pub async fn leaderboard(
    State(state): State<AppState>,
    Query(lb_params): Query<LeaderboardParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let (window, limit) = params::leaderboard(&lb_params)?;
    let key = format!("pool/leaderboard?w={}&l={limit}", window.as_secs());
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let w = resolve_window(window);
        let rows = share_stats::leaderboard(&state.pool, w.since, w.until, limit).await?;
        let pool = share_stats::accepted_pool_wide(&state.pool, w.since).await?;
        let pool_weight = pool.total_weight;
        let entries = rows
            .into_iter()
            .enumerate()
            .map(|(idx, e)| LeaderboardEntryView {
                rank: idx as i64 + 1,
                address: e.address,
                network: e.network,
                accepted_shares: e.accepted_shares,
                hashrate_hs: e.hashrate_hs,
                pool_share: if pool_weight > 0.0 {
                    e.total_weight / pool_weight
                } else {
                    0.0
                },
            })
            .collect();
        to_value(&LeaderboardResponse {
            window_secs: w.secs,
            entries,
        })
    })
    .await
}

/// `GET /api/v1/pool/miners/history` — active-miner count over time.
pub async fn active_miners_history(
    State(state): State<AppState>,
    Query(range_params): Query<RangeParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let range = params::range(&range_params)?;
    let key = format!(
        "pool/miners/history?from={}&to={}&b={}",
        range.from.timestamp(),
        range.until.timestamp(),
        range.bucket.seconds()
    );
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let points = share_stats::active_wallets_series(
            &state.pool,
            range.from,
            range.until,
            range.bucket.seconds(),
        )
        .await?;
        to_value(&ActiveMinersHistory {
            from: range.from,
            to: range.until,
            bucket: bucket_token(range.bucket),
            points: points
                .into_iter()
                .map(|p| ActiveMinersPointView {
                    bucket_start: p.bucket_start,
                    miners: p.miners,
                })
                .collect(),
        })
    })
    .await
}

/// `GET /api/v1/pool/firmware` — miner-software breakdown over a window.
pub async fn firmware(
    State(state): State<AppState>,
    Query(window_params): Query<WindowParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let window = params::window(&window_params)?;
    let key = format!("pool/firmware?w={}", window.as_secs());
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let w = resolve_window(window);
        let rows = connection_session::firmware_breakdown(&state.pool, w.since).await?;
        to_value(&FirmwareBreakdown {
            window_secs: w.secs,
            entries: rows
                .into_iter()
                .map(|r| FirmwareEntryView {
                    app: r.remote_app,
                    workers: r.workers,
                    sessions: r.sessions,
                })
                .collect(),
        })
    })
    .await
}

/// `GET /api/v1/pool/geo` — aggregate miner country distribution.
///
/// Aggregates resolved session countries over a sliding window
/// (ADR-0025). Aggregate-only: no IP, no per-miner geo. Country comes
/// from `MaxMind` `GeoLite2` (attribution required). Returns an empty
/// `entries` array when geo resolution is disabled or unpopulated.
pub async fn geo(
    State(state): State<AppState>,
    Query(window_params): Query<WindowParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let window = params::window(&window_params)?;
    let key = format!("pool/geo?w={}", window.as_secs());
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let w = resolve_window(window);
        let rows = connection_session::country_breakdown(&state.pool, w.since).await?;
        to_value(&GeoBreakdown {
            window_secs: w.secs,
            entries: rows
                .into_iter()
                .map(|r| GeoEntryView {
                    country: r.country,
                    workers: r.workers,
                    sessions: r.sessions,
                })
                .collect(),
        })
    })
    .await
}

/// `GET /api/v1/pool/rejects` — pool-wide reject breakdown by reason.
///
/// Aggregates `share_reject` across all wallets over a sliding window,
/// mirroring the per-miner `rejects` surface. Backs the operator
/// anti-abuse view: which reject reasons dominate pool-wide, right now.
pub async fn rejects(
    State(state): State<AppState>,
    Query(window_params): Query<WindowParams>,
) -> Result<Json<Arc<Value>>, ApiError> {
    let window = params::window(&window_params)?;
    let key = format!("pool/rejects?w={}", window.as_secs());
    let cache = state.pool_cache.clone();
    cached_json(&cache, key, async move {
        let w = resolve_window(window);
        let rows = share_reject::count_by_reason_pool_wide(&state.pool, w.since).await?;
        let total: i64 = rows.iter().map(|(_, count)| *count).sum();
        let by_reason = rows
            .into_iter()
            .map(|(reason, count)| RejectReasonCount::from_row(reason, count))
            .collect();
        to_value(&PoolRejectsResponse {
            window_secs: w.secs,
            total,
            by_reason,
        })
    })
    .await
}

/// The wire token for a bucket width.
pub(crate) const fn bucket_token(bucket: params::Bucket) -> &'static str {
    match bucket {
        params::Bucket::OneMinute => "1m",
        params::Bucket::FiveMinutes => "5m",
        params::Bucket::OneHour => "1h",
        params::Bucket::OneDay => "1d",
    }
}

/// The next keyset cursor: `Some(last_id)` only when the page was full
/// (so there may be more), else `None`.
pub(crate) fn next_cursor(returned: usize, limit: i64, last_id: Option<i64>) -> Option<i64> {
    if i64::try_from(returned).is_ok_and(|n| n >= limit) {
        last_id
    } else {
        None
    }
}
