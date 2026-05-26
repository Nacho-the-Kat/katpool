//! Post-import reconciliation: cross-check `sum(legacy) == sum(new)`
//! for every monetary aggregate that the importer touched.
//!
//! Run after every transform has finished. Mismatches are surfaced
//! as boolean `passed` flags in the [`ReconcileReport`]; the
//! operator decides whether to proceed with cutover or to
//! investigate. The reconcile pass does not abort the importer —
//! a clean reconciliation is a Phase 7 cutover precondition, not a
//! Phase 2 build-time blocker.

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
    /// `legacy == new`. If `false`, the operator investigates.
    pub passed: bool,
}

impl Check {
    const fn from_pair(name: &'static str, legacy: i64, new: i64) -> Self {
        Self {
            name,
            legacy,
            new,
            passed: legacy == new,
        }
    }
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
#[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
pub async fn run(
    source: &sqlx::PgPool,
    target: &sqlx::PgPool,
) -> Result<ReconcileReport, anyhow::Error> {
    info!("starting reconciliation pass");
    let mut checks = Vec::new();

    // ----- blocks ----------------------------------------------------
    let legacy_blocks = source::count_block_details(source).await?;
    let new_blocks = single_i64(target, "SELECT count(*)::bigint FROM block").await?;
    checks.push(Check::from_pair(
        "blocks.row_count",
        legacy_blocks,
        new_blocks,
    ));

    let legacy_reward = source::sum_bigint(source, "block_details", "miner_reward").await?;
    let new_reward = single_i64_opt(target, "SELECT sum(miner_reward_sompi)::bigint FROM block")
        .await?
        .unwrap_or(0);
    checks.push(Check::from_pair(
        "blocks.miner_reward_total_sompi",
        legacy_reward,
        new_reward,
    ));

    // ----- payments ((kas)) -----------------------------------------
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
    checks.push(Check::from_pair(
        "payments.amount_total_sompi",
        legacy_payments_amount,
        new_kas_payouts_amount,
    ));

    // ----- nacho_payments ((krc20)) ---------------------------------
    let legacy_nacho_amount = source::sum_bigint(source, "nacho_payments", "nacho_amount").await?;
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
    checks.push(Check::from_pair(
        "nacho_payments.amount_total",
        legacy_nacho_amount,
        new_nacho_payouts_amount,
    ));

    // ----- miners_balance.nacho_rebate_kas → nacho_rebate_accrual ---
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

    // ----- pending_krc20_transfers (per status, count only) ---------
    for (legacy_status, new_status) in [
        ("PENDING", "pending"),
        ("COMPLETED", "completed"),
        ("FAILED", "failed"),
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
        checks.push(Check::from_pair(
            Box::leak(format!("krc20_pending_transfer.count[{legacy_status}]").into_boxed_str()),
            l,
            n,
        ));
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
            warn!(name = c.name, legacy = c.legacy, new = c.new, "mismatch");
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
