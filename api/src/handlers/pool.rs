//! Pool-wide aggregate handlers, served through the pool TTL cache.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use serde_json::Value;

use katpool_db::repo::{block, payout, share_stats, treasury};

use crate::error::ApiError;
use crate::handlers::{cached_json, resolve_window, to_value};
use crate::models::{
    BlockCounts, BlockView, BlocksPage, CycleView, CyclesPage, HashrateHistory, HashratePointView,
    HashrateSnapshot, PayoutTotals, PoolStats, TreasuryView,
};
use crate::money::KasAmount;
use crate::params::{self, PageParams, RangeParams, WindowParams};
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
