//! Response DTOs — the stable `/api/v1` wire contract.
//!
//! These types own the JSON shape independently of the DB row structs, so a
//! schema rename never silently changes the public surface. All on-chain
//! amounts are [`KasAmount`] (decimal strings); hashrate is a JSON number
//! (a rate, lossy by definition — ADR-0021 §F). Enum labels are mapped here
//! (not via `serde` on the DB enums) so the wire vocabulary is explicit.

use chrono::{DateTime, Utc};
use katpool_db::repo::block::{Block, BlockStatus};
use katpool_db::repo::payout::{
    PayoutCycle, PayoutCycleStatus, PayoutKind, PayoutStatus, WalletPayout,
};
use katpool_db::repo::share_reject::DbShareRejectReason;
use serde::Serialize;

use crate::money::KasAmount;

/// Lowercase hex of a byte slice (block/tx hashes).
fn hex_encode(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Wire label for a block lifecycle status.
#[must_use]
pub const fn block_status_str(status: BlockStatus) -> &'static str {
    match status {
        BlockStatus::Found => "found",
        BlockStatus::SubmittedToNode => "submitted_to_node",
        BlockStatus::ConfirmedBlue => "confirmed_blue",
        BlockStatus::Matured => "matured",
        BlockStatus::Orphaned => "orphaned",
    }
}

/// Wire label for a payout kind (`krc20_nacho` is surfaced as `nacho`).
#[must_use]
pub const fn payout_kind_str(kind: PayoutKind) -> &'static str {
    match kind {
        PayoutKind::Kas => "kas",
        PayoutKind::Krc20Nacho => "nacho",
    }
}

/// Wire label for a per-recipient payout status.
#[must_use]
pub const fn payout_status_str(status: PayoutStatus) -> &'static str {
    match status {
        PayoutStatus::Planned => "planned",
        PayoutStatus::Submitted => "submitted",
        PayoutStatus::Accepted => "accepted",
        PayoutStatus::Confirmed => "confirmed",
        PayoutStatus::Failed => "failed",
    }
}

/// Wire label for a payout cycle status.
#[must_use]
pub const fn cycle_status_str(status: PayoutCycleStatus) -> &'static str {
    match status {
        PayoutCycleStatus::Planned => "planned",
        PayoutCycleStatus::Broadcasting => "broadcasting",
        PayoutCycleStatus::PartiallySettled => "partially_settled",
        PayoutCycleStatus::Settled => "settled",
        PayoutCycleStatus::Failed => "failed",
    }
}

// ---- health / readiness ---------------------------------------------

/// `GET /health` body.
#[derive(Debug, Serialize)]
pub struct Health {
    /// Always `"ok"`.
    pub status: &'static str,
    /// Crate version.
    pub version: &'static str,
}

/// `GET /ready` body.
#[derive(Debug, Serialize)]
pub struct Readiness {
    /// `true` only if DB reachable **and** kaspad synced.
    pub ready: bool,
    /// A recent `SELECT 1` succeeded.
    pub db_reachable: bool,
    /// kaspad reports synced.
    pub kaspad_synced: bool,
}

/// `GET /started` body.
#[derive(Debug, Serialize)]
pub struct Started {
    /// Initial startup completed at least once.
    pub started: bool,
    /// Process start time.
    pub started_at: DateTime<Utc>,
    /// Seconds since process start.
    pub uptime_secs: i64,
}

// ---- pool ------------------------------------------------------------

/// Block counts by lifecycle status, zero-filled.
#[derive(Debug, Default, Serialize)]
pub struct BlockCounts {
    /// Detected but not yet submitted.
    pub found: i64,
    /// Accepted by kaspad.
    pub submitted_to_node: i64,
    /// Confirmed blue in the DAG.
    pub confirmed_blue: i64,
    /// Coinbase matured.
    pub matured: i64,
    /// Displaced by a re-org.
    pub orphaned: i64,
}

impl BlockCounts {
    /// Build from a sparse `(status, count)` list.
    #[must_use]
    pub fn from_rows(rows: &[(BlockStatus, i64)]) -> Self {
        let mut counts = Self::default();
        for (status, count) in rows {
            match status {
                BlockStatus::Found => counts.found = *count,
                BlockStatus::SubmittedToNode => counts.submitted_to_node = *count,
                BlockStatus::ConfirmedBlue => counts.confirmed_blue = *count,
                BlockStatus::Matured => counts.matured = *count,
                BlockStatus::Orphaned => counts.orphaned = *count,
            }
        }
        counts
    }
}

/// Pool-wide confirmed-payout totals.
#[derive(Debug, Serialize)]
pub struct PayoutTotals {
    /// Total confirmed KAS paid.
    pub kas_confirmed: KasAmount,
    /// Total confirmed NACHO payouts, in their KAS-sompi value.
    pub nacho_confirmed: KasAmount,
    /// Count of confirmed payout rows.
    pub confirmed_payouts: i64,
}

/// Latest treasury snapshot (operator-facing balances).
#[derive(Debug, Serialize)]
pub struct TreasuryView {
    /// Snapshot capture time.
    pub captured_at: DateTime<Utc>,
    /// Hot-wallet KAS balance.
    pub kas_balance: KasAmount,
    /// Hot-wallet NACHO balance, integer base units as a string.
    pub nacho_balance: String,
    /// Chain DAA score at capture.
    pub daa_score: i64,
    /// Chain blue score at capture.
    pub blue_score: i64,
}

/// `GET /api/v1/pool/stats` body.
#[derive(Debug, Serialize)]
pub struct PoolStats {
    /// Sliding window the share/hashrate figures cover, in seconds.
    pub window_secs: u64,
    /// Distinct wallets active in the window.
    pub miners_active: i64,
    /// Distinct workers active in the window.
    pub workers_active: i64,
    /// Estimated pool hashrate over the window (H/s).
    pub hashrate_hs: f64,
    /// Accepted shares in the window.
    pub accepted_shares: i64,
    /// Block counts by lifecycle status.
    pub blocks: BlockCounts,
    /// Confirmed-payout totals.
    pub payouts: PayoutTotals,
    /// Latest treasury snapshot, if any has been captured.
    pub treasury: Option<TreasuryView>,
}

/// `GET /api/v1/pool/hashrate` body (also reused per-miner).
#[derive(Debug, Serialize)]
pub struct HashrateSnapshot {
    /// Estimated hashrate (H/s).
    pub hashrate_hs: f64,
    /// Window length in seconds.
    pub window_secs: u64,
}

/// One point of a hashrate time-series.
#[derive(Debug, Serialize)]
pub struct HashratePointView {
    /// Bucket start (UTC).
    pub bucket_start: DateTime<Utc>,
    /// Estimated hashrate over the bucket (H/s).
    pub hashrate_hs: f64,
}

/// A hashrate time-series response (pool-wide or per-miner).
#[derive(Debug, Serialize)]
pub struct HashrateHistory {
    /// Inclusive range start (UTC).
    pub from: DateTime<Utc>,
    /// Exclusive range end (UTC).
    pub to: DateTime<Utc>,
    /// Bucket width token (`1m`/`5m`/`1h`/`1d`).
    pub bucket: &'static str,
    /// Non-empty buckets, ascending by time.
    pub points: Vec<HashratePointView>,
}

/// A block in a list response.
#[derive(Debug, Serialize)]
pub struct BlockView {
    /// Block primary key (also the keyset cursor).
    pub id: i64,
    /// Block hash, lowercase hex.
    pub hash: String,
    /// Lifecycle status.
    pub status: &'static str,
    /// DAA score.
    pub daa_score: i64,
    /// Blue score, once confirmed.
    pub blue_score: Option<i64>,
    /// When the candidate was found.
    pub found_at: DateTime<Utc>,
    /// When confirmed blue.
    pub confirmed_at: Option<DateTime<Utc>>,
    /// When the coinbase matured.
    pub matured_at: Option<DateTime<Utc>>,
    /// Coinbase reward, once matured.
    pub reward: Option<KasAmount>,
}

impl From<&Block> for BlockView {
    fn from(b: &Block) -> Self {
        Self {
            id: b.id.0,
            hash: hex_encode(&b.hash),
            status: block_status_str(b.status),
            daa_score: b.daa_score,
            blue_score: b.blue_score,
            found_at: b.found_at,
            confirmed_at: b.confirmed_at,
            matured_at: b.matured_at,
            reward: b.miner_reward_sompi.map(KasAmount::from_sompi),
        }
    }
}

/// A keyset-paginated page of blocks.
#[derive(Debug, Serialize)]
pub struct BlocksPage {
    /// Blocks, newest-first.
    pub blocks: Vec<BlockView>,
    /// Cursor for the next page (`before=`), or `null` if exhausted.
    pub next_before: Option<i64>,
}

/// A payout cycle in a list response.
#[derive(Debug, Serialize)]
pub struct CycleView {
    /// Cycle primary key (also the keyset cursor).
    pub id: i64,
    /// `kas` or `nacho`.
    pub kind: &'static str,
    /// Cycle status.
    pub status: &'static str,
    /// DAA range start (inclusive).
    pub daa_start: i64,
    /// DAA range end (exclusive).
    pub daa_end: i64,
    /// When the cycle was planned.
    pub planned_at: DateTime<Utc>,
    /// When the last recipient settled.
    pub settled_at: Option<DateTime<Utc>>,
    /// Total amount across recipients.
    pub total: KasAmount,
    /// Recipient count.
    pub total_recipients: i32,
}

impl From<&PayoutCycle> for CycleView {
    fn from(c: &PayoutCycle) -> Self {
        Self {
            id: c.id,
            kind: payout_kind_str(c.kind),
            status: cycle_status_str(c.status),
            daa_start: c.daa_start,
            daa_end: c.daa_end,
            planned_at: c.planned_at,
            settled_at: c.settled_at,
            total: KasAmount::from_sompi(c.total_sompi),
            total_recipients: c.total_recipients,
        }
    }
}

/// A keyset-paginated page of payout cycles.
#[derive(Debug, Serialize)]
pub struct CyclesPage {
    /// Cycles, newest-first.
    pub cycles: Vec<CycleView>,
    /// Cursor for the next page, or `null` if exhausted.
    pub next_before: Option<i64>,
}

/// One entry of the pool leaderboard.
#[derive(Debug, Serialize)]
pub struct LeaderboardEntryView {
    /// 1-based rank within this response (by window hashrate, descending).
    pub rank: i64,
    /// Miner wallet address.
    pub address: String,
    /// Network the wallet was seen on.
    pub network: String,
    /// Accepted shares in the window.
    pub accepted_shares: i64,
    /// Estimated hashrate over the window (H/s).
    pub hashrate_hs: f64,
    /// Fraction of the pool's windowed weight this miner contributed,
    /// in `[0, 1]` (`0` when the pool had no shares in the window).
    pub pool_share: f64,
}

/// `GET /api/v1/pool/leaderboard` body — top miners by window hashrate.
#[derive(Debug, Serialize)]
pub struct LeaderboardResponse {
    /// Sliding window the figures cover, in seconds.
    pub window_secs: u64,
    /// Ranked entries, descending by hashrate.
    pub entries: Vec<LeaderboardEntryView>,
}

/// One point of the active-miners time-series.
#[derive(Debug, Serialize)]
pub struct ActiveMinersPointView {
    /// Bucket start (UTC).
    pub bucket_start: DateTime<Utc>,
    /// Distinct active wallets in the bucket.
    pub miners: i64,
}

/// `GET /api/v1/pool/miners/history` body — active-miner count over time.
#[derive(Debug, Serialize)]
pub struct ActiveMinersHistory {
    /// Inclusive range start (UTC).
    pub from: DateTime<Utc>,
    /// Exclusive range end (UTC).
    pub to: DateTime<Utc>,
    /// Bucket width token (`1m`/`5m`/`1h`/`1d`).
    pub bucket: &'static str,
    /// Non-empty buckets, ascending by time.
    pub points: Vec<ActiveMinersPointView>,
}

/// One slice of the firmware / user-agent breakdown.
#[derive(Debug, Serialize)]
pub struct FirmwareEntryView {
    /// Reported stratum user-agent, or `null` when the client sent none.
    pub app: Option<String>,
    /// Distinct workers reporting this user-agent in the window.
    pub workers: i64,
    /// Sessions opened with this user-agent in the window.
    pub sessions: i64,
}

/// `GET /api/v1/pool/firmware` body — miner-software breakdown.
#[derive(Debug, Serialize)]
pub struct FirmwareBreakdown {
    /// Sliding window the breakdown covers, in seconds.
    pub window_secs: u64,
    /// Slices, descending by worker count.
    pub entries: Vec<FirmwareEntryView>,
}

// ---- miner -----------------------------------------------------------

/// A wallet's KAS payable position.
#[derive(Debug, Serialize)]
pub struct KasBalanceView {
    /// Lifetime allocated.
    pub allocated: KasAmount,
    /// Lifetime confirmed paid.
    pub paid: KasAmount,
    /// Unpaid (allocated − paid).
    pub payable: KasAmount,
}

/// A wallet's NACHO rebate position (KAS-sompi denominated).
#[derive(Debug, Serialize)]
pub struct NachoRebateView {
    /// Cumulative accrued.
    pub accrued: KasAmount,
    /// Cumulative paid.
    pub paid: KasAmount,
    /// Pending (accrued − paid).
    pub pending: KasAmount,
}

impl NachoRebateView {
    /// A zero rebate position, for wallets that never accrued.
    #[must_use]
    pub fn zero() -> Self {
        Self {
            accrued: KasAmount::from_sompi(0),
            paid: KasAmount::from_sompi(0),
            pending: KasAmount::from_sompi(0),
        }
    }
}

/// `GET /api/v1/balance/:address` body.
#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    /// The full address as supplied.
    pub address: String,
    /// Network the wallet was seen on.
    pub network: String,
    /// KAS payable position.
    pub kas: KasBalanceView,
    /// NACHO rebate position.
    pub nacho_rebate: NachoRebateView,
}

/// `GET /api/v1/miners/:address` body.
#[derive(Debug, Serialize)]
pub struct MinerProfile {
    /// The full address as supplied.
    pub address: String,
    /// Network the wallet was seen on.
    pub network: String,
    /// First time the wallet was observed.
    pub first_seen_at: DateTime<Utc>,
    /// Last time the wallet was observed.
    pub last_seen_at: DateTime<Utc>,
    /// Sliding window the share/hashrate figures cover, in seconds.
    pub window_secs: u64,
    /// Accepted shares in the window.
    pub accepted_shares: i64,
    /// Rejected shares in the window.
    pub rejected_shares: i64,
    /// Estimated hashrate over the window (H/s).
    pub hashrate_hs: f64,
    /// Number of workers ever seen for this wallet.
    pub workers_count: usize,
    /// KAS payable position.
    pub kas: KasBalanceView,
    /// NACHO rebate position.
    pub nacho_rebate: NachoRebateView,
}

/// Per-worker activity in the workers response.
#[derive(Debug, Serialize)]
pub struct WorkerView {
    /// Worker label.
    pub name: String,
    /// First time seen.
    pub first_seen_at: DateTime<Utc>,
    /// Last time seen.
    pub last_seen_at: DateTime<Utc>,
    /// Accepted shares in the window.
    pub accepted_shares: i64,
    /// Estimated hashrate over the window (H/s).
    pub hashrate_hs: f64,
}

/// `GET /api/v1/miners/:address/workers` body.
#[derive(Debug, Serialize)]
pub struct WorkersResponse {
    /// The full address as supplied.
    pub address: String,
    /// Sliding window in seconds.
    pub window_secs: u64,
    /// Per-worker breakdown, newest-active first.
    pub workers: Vec<WorkerView>,
}

/// A payout in a per-miner history response.
#[derive(Debug, Serialize)]
pub struct MinerPayoutView {
    /// Payout primary key (also the keyset cursor).
    pub id: i64,
    /// Owning cycle id.
    pub cycle_id: i64,
    /// `kas` or `nacho`.
    pub kind: &'static str,
    /// Payout amount.
    pub amount: KasAmount,
    /// Per-recipient status.
    pub status: &'static str,
    /// KAS tx hash (hex), if submitted.
    pub tx_hash: Option<String>,
    /// KRC-20 commit tx hash (hex), if submitted.
    pub krc20_commit_hash: Option<String>,
    /// KRC-20 reveal tx hash (hex), if submitted.
    pub krc20_reveal_hash: Option<String>,
    /// When the payout row was created.
    pub planned_at: DateTime<Utc>,
    /// When the tx was submitted.
    pub submitted_at: Option<DateTime<Utc>>,
    /// When the tx confirmed.
    pub confirmed_at: Option<DateTime<Utc>>,
    /// Failure reason, if failed.
    pub failure_reason: Option<String>,
}

impl From<&WalletPayout> for MinerPayoutView {
    fn from(p: &WalletPayout) -> Self {
        Self {
            id: p.id,
            cycle_id: p.cycle_id,
            kind: payout_kind_str(p.kind),
            amount: KasAmount::from_sompi(p.amount_sompi),
            status: payout_status_str(p.status),
            tx_hash: p.tx_hash.as_deref().map(hex_encode),
            krc20_commit_hash: p.krc20_commit_hash.as_deref().map(hex_encode),
            krc20_reveal_hash: p.krc20_reveal_hash.as_deref().map(hex_encode),
            planned_at: p.planned_at,
            submitted_at: p.submitted_at,
            confirmed_at: p.confirmed_at,
            failure_reason: p.failure_reason.clone(),
        }
    }
}

/// `GET /api/v1/miners/:address/payouts` body.
#[derive(Debug, Serialize)]
pub struct MinerPayoutsPage {
    /// The full address as supplied.
    pub address: String,
    /// Payouts, newest-first.
    pub payouts: Vec<MinerPayoutView>,
    /// Cursor for the next page, or `null` if exhausted.
    pub next_before: Option<i64>,
}

/// One reason's reject count.
#[derive(Debug, Serialize)]
pub struct RejectReasonCount {
    /// Reason label (e.g. `low_difficulty`).
    pub reason: &'static str,
    /// Count in the window.
    pub count: i64,
}

/// `GET /api/v1/miners/:address/rejects` body.
#[derive(Debug, Serialize)]
pub struct RejectsResponse {
    /// The full address as supplied.
    pub address: String,
    /// Sliding window in seconds.
    pub window_secs: u64,
    /// Total rejects in the window.
    pub total: i64,
    /// Per-reason breakdown, descending by count.
    pub by_reason: Vec<RejectReasonCount>,
}

impl RejectReasonCount {
    /// Build from a `(reason, count)` row.
    #[must_use]
    pub const fn from_row(reason: DbShareRejectReason, count: i64) -> Self {
        Self {
            reason: reason.as_str(),
            count,
        }
    }
}

/// `GET /api/v1/pool/rejects` body — pool-wide reject breakdown.
#[derive(Debug, Serialize)]
pub struct PoolRejectsResponse {
    /// Sliding window in seconds.
    pub window_secs: u64,
    /// Total rejects across the pool in the window.
    pub total: i64,
    /// Per-reason breakdown, descending by count.
    pub by_reason: Vec<RejectReasonCount>,
}

/// `GET /api/v1/full_rebate/:address` body.
#[derive(Debug, Serialize)]
pub struct FullRebateResponse {
    /// The full address as supplied.
    pub address: String,
    /// Applied tier (`standard`/`elite`), or `null` if never allocated.
    pub tier: Option<&'static str>,
    /// `true` iff the wallet's most-recent allocation was the Elite
    /// (100% rebate) tier.
    pub full_rebate: bool,
}
