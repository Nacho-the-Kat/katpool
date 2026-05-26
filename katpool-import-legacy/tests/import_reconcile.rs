//! End-to-end test of the post-import reconciliation pass.
//!
//! Seeds the legacy DB with one row per source table, runs every
//! transform in order, then runs the reconcile pass and asserts
//! `all_passed = true`. This is the closest thing we have to a
//! "cutover dry run" without real production data.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use chrono::NaiveDateTime;
use sqlx::types::BigDecimal;
use std::str::FromStr;

use katpool_import_legacy::reconcile;
use katpool_import_legacy::transform::{balances, blocks, krc20, nacho_payments, payments};

mod common;
use common::{MINER_A, MINER_B, POOL_ADDR, VALID_HASH_A, VALID_HASH_B, VALID_HASH_C, setup};

fn ts(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

async fn seed_full_legacy(legacy: &sqlx::PgPool) {
    // ---- block_details
    sqlx::query(
        "INSERT INTO block_details (mined_block_hash, miner_id, pool_address, wallet, daa_score, miner_reward)
         VALUES ($1, 'JANKS5Pro', $2, $3, '427337124', 275000000),
                ($4, 'KS5P02', $2, $5, '427337293', 275079404)",
    )
    .bind(VALID_HASH_A)
    .bind(POOL_ADDR)
    .bind(MINER_A)
    .bind(VALID_HASH_B)
    .bind(MINER_B)
    .execute(legacy)
    .await
    .unwrap();

    // ---- miners_balance
    sqlx::query(
        "INSERT INTO miners_balance (id, miner_id, wallet, balance, nacho_rebate_kas)
         VALUES ('wallet-A.JANKS5', 'JANKS5Pro', $1, NULL, $2),
                ('wallet-B.KS5P02', 'KS5P02', $3, NULL, $4)",
    )
    .bind(MINER_A)
    .bind(BigDecimal::from_str("12345").unwrap())
    .bind(MINER_B)
    .bind(BigDecimal::from_str("6789").unwrap())
    .execute(legacy)
    .await
    .unwrap();

    // ---- payments
    sqlx::query(
        "INSERT INTO payments (wallet_address, amount, timestamp, transaction_hash)
         VALUES (ARRAY[$1], 50000, $2, $3),
                (ARRAY[$4], 70000, $2, $3),
                (ARRAY[$1], 30000, $2, $5)",
    )
    .bind(MINER_A)
    .bind(ts("2025-09-01 10:00:00"))
    .bind(VALID_HASH_A)
    .bind(MINER_B)
    .bind(VALID_HASH_B)
    .execute(legacy)
    .await
    .unwrap();

    // ---- nacho_payments
    sqlx::query(
        "INSERT INTO nacho_payments (wallet_address, nacho_amount, timestamp, transaction_hash)
         VALUES (ARRAY[$1], 1000, $2, $3),
                (ARRAY[$4], 2000, $2, $3)",
    )
    .bind(MINER_A)
    .bind(ts("2025-09-01 11:00:00"))
    .bind(VALID_HASH_C)
    .bind(MINER_B)
    .execute(legacy)
    .await
    .unwrap();

    // ---- pending_krc20_transfers
    sqlx::query(
        "INSERT INTO pending_krc20_transfers
            (first_txn_id, sompi_to_miner, nacho_amount, address, p2sh_address, nacho_transfer_status, db_entry_status, timestamp)
         VALUES ('tx-p-1', 1000, 10, $1, 'p2sh:p', 'PENDING', 'PENDING', $2),
                ('tx-c-1', 2000, 20, $3, 'p2sh:c', 'COMPLETED', 'COMPLETED', $2),
                ('tx-f-1', 3000, 30, $1, 'p2sh:f', 'FAILED', 'FAILED', $2)",
    )
    .bind(MINER_A)
    .bind(ts("2025-09-01 12:00:00"))
    .bind(MINER_B)
    .execute(legacy)
    .await
    .unwrap();
}

#[tokio::test]
async fn reconcile_all_passes_after_full_import() {
    let env = setup().await;
    seed_full_legacy(&env.legacy).await;

    let _ = blocks::run(&env.legacy, &env.target, false).await.unwrap();
    let _ = balances::run(&env.legacy, &env.target, false)
        .await
        .unwrap();
    let _ = payments::run(&env.legacy, &env.target, false)
        .await
        .unwrap();
    let _ = nacho_payments::run(&env.legacy, &env.target, false)
        .await
        .unwrap();
    let _ = krc20::run(&env.legacy, &env.target, false).await.unwrap();

    let report = reconcile::run(&env.legacy, &env.target)
        .await
        .expect("reconcile");
    for c in &report.checks {
        assert!(
            c.passed,
            "check {} failed: legacy={} new={}",
            c.name, c.legacy, c.new
        );
    }
    assert!(report.all_passed, "all checks must pass; report={report:?}");

    // Spot-check a few specific aggregates.
    let block_check = report
        .checks
        .iter()
        .find(|c| c.name == "blocks.row_count")
        .unwrap();
    assert_eq!(block_check.legacy, 2);
    assert_eq!(block_check.new, 2);

    let payments_check = report
        .checks
        .iter()
        .find(|c| c.name == "payments.amount_total_sompi")
        .unwrap();
    assert_eq!(payments_check.legacy, 150_000);

    let rebate_check = report
        .checks
        .iter()
        .find(|c| c.name == "miners_balance.nacho_rebate_total")
        .unwrap();
    assert_eq!(rebate_check.legacy, 12345 + 6789);
}

#[tokio::test]
async fn reconcile_fails_when_target_is_empty() {
    let env = setup().await;
    seed_full_legacy(&env.legacy).await;

    // Do NOT run any transforms. Target stays empty; reconcile must fail.
    let report = reconcile::run(&env.legacy, &env.target)
        .await
        .expect("reconcile");
    assert!(!report.all_passed, "must fail when target is empty");
    let failed = report.checks.iter().filter(|c| !c.passed).count();
    assert!(failed > 0);
}
