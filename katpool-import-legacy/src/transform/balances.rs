//! `miners_balance` → `nacho_rebate_accrual` transform.
//!
//! ## Why we don't import `miners_balance.balance` (pending KAS)
//!
//! The new schema computes KAS payable balance from per-block
//! `share_allocation.net_payout_sompi`, which the legacy schema
//! never tracked. The cutover plan
//! (`docs/runbooks/14-legacy-importer.md`) explicitly pays out
//! every remaining legacy `balance` from the old pool as the last
//! act of the legacy stack before the new pool takes over share
//! ingest. This transform is therefore intentionally limited to
//! the NACHO rebate column.
//!
//! The legacy `nacho_rebate_kas` column is the pending (un-paid)
//! NACHO rebate balance for a wallet. We map it to
//! `nacho_rebate_accrual.accrued_sompi` and set
//! `paid_sompi = 0` — the new pool treats every legacy-accrued
//! sompi as still-payable, scheduling it to clear in the next
//! NACHO payout cycle.

use num_traits::cast::ToPrimitive;
use sqlx::types::BigDecimal;
use tracing::{info, warn};

use katpool_db::repo::{nacho_rebate, wallet, worker};
use katpool_domain::{WalletAddress, WorkerName};

use crate::source::{self, LegacyMinersBalance};
use crate::transform::TransformStats;

const LEGACY_NETWORK: &str = "mainnet";

/// Run the `miners_balance` → `nacho_rebate_accrual` transform.
pub async fn run(
    source: &sqlx::PgPool,
    target: &sqlx::PgPool,
    dry_run: bool,
) -> Result<TransformStats, anyhow::Error> {
    let rows = source::fetch_miners_balance(source).await?;
    info!(
        row_count = rows.len(),
        dry_run, "starting miners_balance import"
    );

    let mut stats = TransformStats::default();
    for row in rows {
        stats.read += 1;
        match import_one(target, &row, dry_run).await {
            Ok(Outcome::Set) => stats.inserted += 1,
            Ok(Outcome::Zero) => stats.skipped += 1,
            Ok(Outcome::Rejected(reason)) => {
                stats.rejected += 1;
                warn!(id = %row.id, reason, "miners_balance row rejected");
            }
            Err(e) => return Err(e.context(format!("import miners_balance id={}", row.id))),
        }
    }

    info!(stats = %stats, "miners_balance import complete");
    Ok(stats)
}

#[derive(Debug)]
enum Outcome {
    /// Wrote an `accrued_sompi` value > 0.
    Set,
    /// Row had zero rebate; nothing to write (counted as `skipped`).
    Zero,
    /// Validation failure.
    Rejected(&'static str),
}

async fn import_one(
    target: &sqlx::PgPool,
    row: &LegacyMinersBalance,
    dry_run: bool,
) -> Result<Outcome, anyhow::Error> {
    let Some(wallet_str) = row.wallet.as_ref() else {
        return Ok(Outcome::Rejected("missing wallet"));
    };
    let Ok(wallet_addr) = WalletAddress::new(wallet_str.clone()) else {
        return Ok(Outcome::Rejected("wallet fails domain validation"));
    };

    let accrued = match decimal_to_i64_or_reject(row.nacho_rebate_kas.as_ref()) {
        Ok(v) => v,
        Err(reason) => return Ok(Outcome::Rejected(reason)),
    };

    // Worker name is optional in the legacy `miners_balance` table
    // and we don't strictly need a worker row for the rebate
    // accrual (the FK is to wallet, not worker). Skip the
    // worker upsert if absent or invalid; the corresponding worker
    // will be created later by `block_details` if the wallet was
    // active.
    let worker_name_opt = row
        .miner_id
        .as_ref()
        .filter(|s| !s.is_empty())
        .and_then(|s| WorkerName::new(s.clone()).ok());

    if dry_run {
        return Ok(if accrued > 0 {
            Outcome::Set
        } else {
            Outcome::Zero
        });
    }

    let mut tx = target.begin().await?;
    let w = wallet::ensure(&mut *tx, &wallet_addr, LEGACY_NETWORK).await?;
    if let Some(wname) = &worker_name_opt {
        let _ = worker::ensure(&mut *tx, w.id, wname).await?;
    }
    if accrued > 0 {
        nacho_rebate::set_accrual(&mut *tx, w.id, accrued).await?;
    }
    tx.commit().await?;

    Ok(if accrued > 0 {
        Outcome::Set
    } else {
        Outcome::Zero
    })
}

/// Coerce a legacy `numeric` (`BigDecimal`) into a signed `i64`.
/// Rejects negative, overflow, or non-integer (with a fractional
/// component) values. The legacy column is `numeric` with no scale,
/// so fractional values shouldn't appear in practice — we check
/// anyway because cheap.
fn decimal_to_i64_or_reject(d: Option<&BigDecimal>) -> Result<i64, &'static str> {
    let Some(d) = d else {
        return Ok(0);
    };
    let Some(v) = d.to_i64() else {
        return Err("nacho_rebate_kas overflows i64");
    };
    if v < 0 {
        return Err("nacho_rebate_kas negative");
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn decimal_zero_when_absent() {
        assert_eq!(decimal_to_i64_or_reject(None).unwrap(), 0);
    }

    #[test]
    fn decimal_passes_through_positive() {
        let d = BigDecimal::from_str("12345").unwrap();
        assert_eq!(decimal_to_i64_or_reject(Some(&d)).unwrap(), 12345);
    }

    #[test]
    fn decimal_rejects_negative() {
        let d = BigDecimal::from_str("-1").unwrap();
        assert_eq!(
            decimal_to_i64_or_reject(Some(&d)),
            Err("nacho_rebate_kas negative")
        );
    }

    #[test]
    fn decimal_rejects_overflow() {
        let d = BigDecimal::from_str("999999999999999999999").unwrap();
        assert_eq!(
            decimal_to_i64_or_reject(Some(&d)),
            Err("nacho_rebate_kas overflows i64")
        );
    }
}
