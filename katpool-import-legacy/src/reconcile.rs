//! Post-import reconciliation: cross-check `sum(legacy) == sum(new)`
//! for every monetary aggregate that the importer touched.
//!
//! Run after every transform has finished. Mismatches are surfaced
//! as boolean `passed` flags in the [`ReconcileReport`]; the
//! operator decides whether to proceed with cutover or to
//! investigate. The reconcile pass does not abort the importer —
//! a clean reconciliation is a Phase 7 cutover precondition, not a
//! Phase 2 build-time blocker.

use std::collections::HashSet;

use num_traits::cast::ToPrimitive;
use tracing::{info, warn};

use crate::source;

/// Single cross-table aggregate check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Check {
    /// Operator-readable name (`blocks.miner_reward_total_sompi`).
    pub name: &'static str,
    /// Aggregate on the legacy source.
    pub legacy: i64,
    /// Aggregate on the new target.
    pub new: i64,
    /// Expected, accounted-for legacy→new shortfall: rows the importer
    /// **intentionally** dropped because they fail the new schema's validation
    /// (e.g. a malformed legacy wallet/worker). `0` for an exact check.
    pub allowance: i64,
    /// `legacy == new + allowance`. If `false`, the operator investigates.
    pub passed: bool,
}

impl Check {
    /// An exact check (`legacy == new`).
    const fn from_pair(name: &'static str, legacy: i64, new: i64) -> Self {
        Self::with_allowance(name, legacy, new, 0)
    }

    /// A check that tolerates a known, accounted-for `allowance` — the
    /// aggregate of the rows the transform rejected as invalid. Passes iff
    /// `legacy == new + allowance`, so the only permitted divergence is exactly
    /// the documented rejects (anything else still fails the gate).
    const fn with_allowance(name: &'static str, legacy: i64, new: i64, allowance: i64) -> Self {
        Self {
            name,
            legacy,
            new,
            allowance,
            passed: legacy == new + allowance,
        }
    }
}

/// Accounted-for reject allowances threaded from the transforms.
///
/// The reconcile passes when the only legacy→new shortfall is exactly the rows
/// the importer intentionally dropped (invalid wallet/worker), and fails on any
/// other divergence.
#[derive(Debug, Clone, Copy, Default)]
pub struct Allowances {
    /// `block_details` rows rejected (count).
    pub blocks_count: i64,
    /// Sum of `miner_reward` over the rejected `block_details` rows.
    pub blocks_reward_sompi: i64,
    /// Sompi over `payments` rows rejected or collapsed by a within-cycle
    /// duplicate wallet (`rejected_amount + deduped_amount`).
    pub payments_sompi: i64,
    /// NACHO base units over `nacho_payments` rows rejected or collapsed by a
    /// within-cycle duplicate wallet (`rejected_amount + deduped_amount`).
    pub nacho_amount: i64,
    /// `pending_krc20_transfers` rejects, by legacy status.
    pub krc20_pending: i64,
    /// `pending_krc20_transfers` rejects in COMPLETED state.
    pub krc20_completed: i64,
    /// `pending_krc20_transfers` rejects in FAILED state.
    pub krc20_failed: i64,
}

/// Full reconciliation report. Serialised into the importer's
/// stdout JSON envelope so the cutover runbook captures it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReconcileReport {
    /// Every cross-aggregate check, in declaration order.
    pub checks: Vec<Check>,
    /// `true` iff every `Check.passed` is `true`.
    pub all_passed: bool,
}

/// Execute every reconcile check against both pools.
#[allow(
    clippy::cognitive_complexity,
    clippy::too_many_lines,
    clippy::implicit_hasher
)]
pub async fn run(
    source: &sqlx::PgPool,
    target: &sqlx::PgPool,
    allow: &Allowances,
    skip: &HashSet<String>,
) -> Result<ReconcileReport, anyhow::Error> {
    info!("starting reconciliation pass");
    let mut checks = Vec::new();

    // ----- blocks ----------------------------------------------------
    // Block rows can be rejected (invalid legacy wallet/worker), so tolerate
    // exactly the rejected count + their reward sum — any other gap still fails.
    // Skipped when the blocks transform was deferred (e.g. backfilled after
    // promote): a stale `block` count would otherwise fail a meaningless check.
    if !skip.contains("blocks") {
        let legacy_blocks = source::count_block_details(source).await?;
        let new_blocks = single_i64(target, "SELECT count(*)::bigint FROM block").await?;
        checks.push(Check::with_allowance(
            "blocks.row_count",
            legacy_blocks,
            new_blocks,
            allow.blocks_count,
        ));

        let legacy_reward = source::sum_bigint(source, "block_details", "miner_reward").await?;
        let new_reward =
            single_i64_opt(target, "SELECT sum(miner_reward_sompi)::bigint FROM block")
                .await?
                .unwrap_or(0);
        checks.push(Check::with_allowance(
            "blocks.miner_reward_total_sompi",
            legacy_reward,
            new_reward,
            allow.blocks_reward_sompi,
        ));
    }

    // ----- payments ((kas)) -----------------------------------------
    if !skip.contains("payments") {
        let legacy_payments_amount = source::sum_bigint(source, "payments", "amount").await?;
        let new_kas_payouts_amount = single_i64_opt(
            target,
            "SELECT sum(p.amount_sompi)::bigint
               FROM payout p
               JOIN payout_cycle c ON c.id = p.cycle_id
              WHERE c.kind = 'kas'
                AND c.idempotency_key LIKE 'kas-legacy-%'",
        )
        .await?
        .unwrap_or(0);
        checks.push(Check::with_allowance(
            "payments.amount_total_sompi",
            legacy_payments_amount,
            new_kas_payouts_amount,
            allow.payments_sompi,
        ));
    }

    // ----- nacho_payments ((krc20)) ---------------------------------
    if !skip.contains("nacho_payments") {
        let legacy_nacho_amount =
            source::sum_bigint(source, "nacho_payments", "nacho_amount").await?;
        let new_nacho_payouts_amount = single_i64_opt(
            target,
            "SELECT sum(p.amount_sompi)::bigint
               FROM payout p
               JOIN payout_cycle c ON c.id = p.cycle_id
              WHERE c.kind = 'krc20_nacho'
                AND c.idempotency_key LIKE 'krc20-legacy-%'
                AND c.idempotency_key NOT LIKE 'krc20-legacy-pending-%'",
        )
        .await?
        .unwrap_or(0);
        checks.push(Check::with_allowance(
            "nacho_payments.amount_total",
            legacy_nacho_amount,
            new_nacho_payouts_amount,
            allow.nacho_amount,
        ));
    }

    // ----- miners_balance.nacho_rebate_kas → nacho_rebate_accrual ---
    if !skip.contains("balances") {
        let legacy_rebate = source::sum_numeric(source, "miners_balance", "nacho_rebate_kas")
            .await?
            .to_i64()
            .unwrap_or_default();
        let new_accrued = single_i64_opt(
            target,
            "SELECT sum(accrued_sompi)::bigint FROM nacho_rebate_accrual",
        )
        .await?
        .unwrap_or(0);
        checks.push(Check::from_pair(
            "miners_balance.nacho_rebate_total",
            legacy_rebate,
            new_accrued,
        ));
    }

    // ----- pending_krc20_transfers (per status, count only) ---------
    // Each status tolerates its own rejected count (invalid legacy rows).
    if !skip.contains("krc20") {
        for (legacy_status, new_status, status_allowance) in [
            ("PENDING", "pending", allow.krc20_pending),
            ("COMPLETED", "completed", allow.krc20_completed),
            ("FAILED", "failed", allow.krc20_failed),
        ] {
            let l = single_i64(
                source,
                // safe interpolation — status is a static literal
                &format!(
                    "SELECT count(*)::bigint FROM pending_krc20_transfers \
                     WHERE nacho_transfer_status = '{legacy_status}'"
                ),
            )
            .await?;
            let n = single_i64(
                target,
                &format!(
                    "SELECT count(*)::bigint FROM krc20_pending_transfer WHERE status = '{new_status}'"
                ),
            )
            .await?;
            checks.push(Check::with_allowance(
                Box::leak(
                    format!("krc20_pending_transfer.count[{legacy_status}]").into_boxed_str(),
                ),
                l,
                n,
                status_allowance,
            ));
        }
    }

    let all_passed = checks.iter().all(|c| c.passed);
    if all_passed {
        info!(
            checks = checks.len(),
            "reconciliation pass: every check matched"
        );
    } else {
        warn!(
            failed = checks.iter().filter(|c| !c.passed).count(),
            total = checks.len(),
            "reconciliation pass: one or more checks did not match — investigate before cutover"
        );
        for c in checks.iter().filter(|c| !c.passed) {
            warn!(
                name = c.name,
                legacy = c.legacy,
                new = c.new,
                allowance = c.allowance,
                "mismatch"
            );
        }
    }

    Ok(ReconcileReport { checks, all_passed })
}

async fn single_i64(pool: &sqlx::PgPool, sql: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(sql).fetch_one(pool).await
}

async fn single_i64_opt(pool: &sqlx::PgPool, sql: &str) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(sql).fetch_one(pool).await
}

#[cfg(test)]
mod tests {
    use super::Check;

    #[test]
    fn exact_check_passes_only_on_equality() {
        assert!(Check::from_pair("x", 100, 100).passed);
        assert!(!Check::from_pair("x", 100, 99).passed);
    }

    #[test]
    fn allowance_tolerates_exactly_the_rejected_shortfall() {
        // new is short by exactly the allowance (rejected rows) → passes.
        assert!(Check::with_allowance("x", 100, 94, 6).passed);
        // short by more than the allowance → still fails (real data loss).
        assert!(!Check::with_allowance("x", 100, 93, 6).passed);
        // new exceeds legacy-minus-allowance → fails (unexpected surplus).
        assert!(!Check::with_allowance("x", 100, 95, 6).passed);
        // zero allowance behaves like an exact check.
        assert!(Check::with_allowance("x", 100, 100, 0).passed);
    }
}
