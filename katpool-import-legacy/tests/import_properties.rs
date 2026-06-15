//! Cross-cutting property tests for the legacy importer.
//!
//! Per-transform happy-path tests already cover the basic insert
//! / skip / reject paths. This file exercises *invariants that
//! span transforms or simulate partial-failure restarts*:
//!
//! 1. **Idempotent rerun with new rows added.** Import N rows,
//!    then add M new rows to legacy, then re-import → final
//!    target state contains exactly N+M imported rows and the
//!    reconcile pass converges. Simulates the realistic case
//!    where the operator runs the importer in the dry-run window
//!    first, the legacy pool keeps writing, and the cutover
//!    import picks up the new rows.
//!
//! 2. **SET-not-ADD on rebate.** Two `set_accrual` calls with
//!    different values produce the **second** value as the final
//!    state, not their sum. Critical for re-runnability.
//!
//! 3. **Partial-failure restart safety.** Simulate a mid-import
//!    crash by importing only one of two batches, then importing
//!    everything → final state equals direct one-shot import.
//!
//! 4. **Reconcile catches data-side mutation between snapshot and
//!    cutover.** If the legacy DB grows new payments after the
//!    importer ran, the reconcile pass surfaces the mismatch.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use chrono::NaiveDateTime;
use katpool_db::repo::{nacho_rebate, wallet};
use katpool_domain::WalletAddress;
use sqlx::types::BigDecimal;
use std::str::FromStr;

use katpool_import_legacy::reconcile;
use katpool_import_legacy::transform::{balances, blocks, krc20, nacho_payments, payments};

mod common;
use common::{MINER_A, MINER_B, POOL_ADDR, VALID_HASH_A, VALID_HASH_B, VALID_HASH_C, setup};

fn ts(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

async fn seed_block(
    legacy: &sqlx::PgPool,
    mined_block_hash: &str,
    wallet: &str,
    worker: &str,
    miner_reward: i64,
) {
    sqlx::query(
        "INSERT INTO block_details
            (mined_block_hash, miner_id, pool_address, wallet, daa_score, miner_reward)
         VALUES ($1, $2, $3, $4, '427337124', $5)",
    )
    .bind(mined_block_hash)
    .bind(worker)
    .bind(POOL_ADDR)
    .bind(wallet)
    .bind(miner_reward)
    .execute(legacy)
    .await
    .expect("seed block_details");
}

async fn seed_payment(
    legacy: &sqlx::PgPool,
    wallet: &str,
    amount: i64,
    tx_hash: &str,
    timestamp: NaiveDateTime,
) {
    sqlx::query(
        "INSERT INTO payments (wallet_address, amount, timestamp, transaction_hash)
         VALUES (ARRAY[$1], $2, $3, $4)",
    )
    .bind(wallet)
    .bind(amount)
    .bind(timestamp)
    .bind(tx_hash)
    .execute(legacy)
    .await
    .expect("seed payments");
}

async fn count_target_blocks(target: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*)::bigint FROM block")
        .fetch_one(target)
        .await
        .unwrap()
}

async fn count_target_payouts(target: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*)::bigint FROM payout")
        .fetch_one(target)
        .await
        .unwrap()
}

// ---------------------------------------------------------------
// Property 1: idempotent rerun with new rows added
// ---------------------------------------------------------------

#[tokio::test]
async fn rerun_with_new_rows_picks_up_only_new_data() {
    let env = setup().await;

    // Initial batch: 2 blocks, 1 payments cycle.
    seed_block(&env.legacy, VALID_HASH_A, MINER_A, "worker-1", 275_000_000).await;
    seed_block(&env.legacy, VALID_HASH_B, MINER_B, "worker-2", 275_000_000).await;
    seed_payment(
        &env.legacy,
        MINER_A,
        50_000,
        VALID_HASH_A,
        ts("2025-09-01 10:00:00"),
    )
    .await;

    let blocks_1 = blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("blocks run1");
    let payments_1 = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("payments run1");
    assert_eq!(blocks_1.inserted, 2);
    assert_eq!(payments_1.inserted, 1);
    assert_eq!(count_target_blocks(&env.target).await, 2);

    // Legacy grows by 1 block + 1 payment under a NEW tx_hash.
    seed_block(&env.legacy, VALID_HASH_C, MINER_A, "worker-1", 275_000_000).await;
    seed_payment(
        &env.legacy,
        MINER_B,
        70_000,
        VALID_HASH_C,
        ts("2025-09-01 11:00:00"),
    )
    .await;

    let blocks_2 = blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("blocks run2");
    let payments_2 = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("payments run2");

    // Re-run sees 3 blocks (all 3 read), 2 already-existing (skipped), 1 new (inserted).
    assert_eq!(blocks_2.read, 3);
    assert_eq!(blocks_2.inserted, 1);
    assert_eq!(blocks_2.skipped, 2);

    // For payments, the existing cycle's row is skipped; the new cycle inserts 1.
    assert_eq!(payments_2.read, 2);
    assert_eq!(payments_2.inserted, 1);

    // Final state: 3 blocks, 2 payouts.
    assert_eq!(count_target_blocks(&env.target).await, 3);
    assert_eq!(count_target_payouts(&env.target).await, 2);

    // Reconcile passes against the post-grow legacy DB.
    let report = reconcile::run(&env.legacy, &env.target, &reconcile::Allowances::default())
        .await
        .expect("reconcile");
    assert!(
        report.all_passed,
        "reconcile should pass after rerun-with-additions; report={report:?}"
    );
}

// ---------------------------------------------------------------
// Property 2: set-not-add on rebate
// ---------------------------------------------------------------

#[tokio::test]
async fn rebate_set_semantics_overwrite_not_accumulate() {
    let env = setup().await;
    let wallet_addr = WalletAddress::new(MINER_A.to_owned()).unwrap();

    // Use the repo function directly so the test isolates the
    // SET-not-ADD invariant from the transform's other logic.
    let mut tx = env.target.begin().await.unwrap();
    let w = wallet::ensure(&mut *tx, &wallet_addr, "mainnet")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // First set: 1000.
    nacho_rebate::set_accrual(&env.target, w.id, 1000)
        .await
        .unwrap();
    let r1 = nacho_rebate::get(&env.target, w.id).await.unwrap().unwrap();
    assert_eq!(r1.accrued_sompi, 1000);

    // Second set: 700 (lower than first). Must overwrite, not accumulate.
    nacho_rebate::set_accrual(&env.target, w.id, 700)
        .await
        .unwrap();
    let r2 = nacho_rebate::get(&env.target, w.id).await.unwrap().unwrap();
    assert_eq!(r2.accrued_sompi, 700);
    assert_eq!(r2.paid_sompi, 0);

    // Third set: 2500. Same semantics.
    nacho_rebate::set_accrual(&env.target, w.id, 2500)
        .await
        .unwrap();
    let r3 = nacho_rebate::get(&env.target, w.id).await.unwrap().unwrap();
    assert_eq!(r3.accrued_sompi, 2500);

    // Negative rejected.
    let err = nacho_rebate::set_accrual(&env.target, w.id, -1)
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("non-negative"), "unexpected error: {msg}");
}

#[tokio::test]
async fn rebate_set_through_transform_is_idempotent() {
    let env = setup().await;
    sqlx::query(
        "INSERT INTO miners_balance (id, miner_id, wallet, balance, nacho_rebate_kas)
         VALUES ('w-1', 'worker-1', $1, NULL, $2)",
    )
    .bind(MINER_A)
    .bind(BigDecimal::from_str("1000").unwrap())
    .execute(&env.legacy)
    .await
    .unwrap();

    // First import.
    let _ = balances::run(&env.legacy, &env.target, false)
        .await
        .unwrap();
    let wallet_addr = WalletAddress::new(MINER_A.to_owned()).unwrap();
    let w = wallet::find_by_address(&env.target, &wallet_addr)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        nacho_rebate::get(&env.target, w.id)
            .await
            .unwrap()
            .unwrap()
            .accrued_sompi,
        1000
    );

    // Mutate legacy: cut the rebate in half (as would happen if the
    // legacy pool paid a partial NACHO cycle between imports).
    sqlx::query("UPDATE miners_balance SET nacho_rebate_kas = $1 WHERE id = 'w-1'")
        .bind(BigDecimal::from_str("500").unwrap())
        .execute(&env.legacy)
        .await
        .unwrap();

    // Re-import. New value, not sum.
    let _ = balances::run(&env.legacy, &env.target, false)
        .await
        .unwrap();
    assert_eq!(
        nacho_rebate::get(&env.target, w.id)
            .await
            .unwrap()
            .unwrap()
            .accrued_sompi,
        500
    );
}

// ---------------------------------------------------------------
// Property 3: partial-failure restart safety
// ---------------------------------------------------------------

#[tokio::test]
async fn partial_failure_restart_converges() {
    // Simulate a crash between transforms. The operator restarts
    // the importer; the second run must converge to the same
    // final state as if the first run had finished.
    let env = setup().await;

    // Seed full dataset.
    seed_block(&env.legacy, VALID_HASH_A, MINER_A, "w-1", 275_000_000).await;
    seed_block(&env.legacy, VALID_HASH_B, MINER_B, "w-2", 275_000_000).await;
    seed_payment(
        &env.legacy,
        MINER_A,
        100,
        VALID_HASH_A,
        ts("2025-09-01 10:00:00"),
    )
    .await;

    // "Crash" after blocks but before payments.
    let _ = blocks::run(&env.legacy, &env.target, false).await.unwrap();

    // Restart: re-run everything. blocks skipped, payments inserted.
    let blocks2 = blocks::run(&env.legacy, &env.target, false).await.unwrap();
    let payments2 = payments::run(&env.legacy, &env.target, false)
        .await
        .unwrap();
    assert_eq!(blocks2.inserted, 0);
    assert_eq!(blocks2.skipped, 2);
    assert_eq!(payments2.inserted, 1);

    // Reconcile converges.
    let report = reconcile::run(&env.legacy, &env.target, &reconcile::Allowances::default())
        .await
        .expect("reconcile");
    assert!(report.all_passed);
}

// ---------------------------------------------------------------
// Property 4: reconcile detects legacy mutation
// ---------------------------------------------------------------

#[tokio::test]
async fn reconcile_detects_legacy_mutation_after_import() {
    let env = setup().await;

    seed_block(&env.legacy, VALID_HASH_A, MINER_A, "w-1", 275_000_000).await;
    seed_payment(
        &env.legacy,
        MINER_A,
        100,
        VALID_HASH_A,
        ts("2025-09-01 10:00:00"),
    )
    .await;
    let _ = blocks::run(&env.legacy, &env.target, false).await.unwrap();
    let _ = payments::run(&env.legacy, &env.target, false)
        .await
        .unwrap();
    let _ = nacho_payments::run(&env.legacy, &env.target, false)
        .await
        .unwrap();
    let _ = krc20::run(&env.legacy, &env.target, false).await.unwrap();
    let _ = balances::run(&env.legacy, &env.target, false)
        .await
        .unwrap();

    // Pre-mutation, reconcile passes.
    assert!(
        reconcile::run(&env.legacy, &env.target, &reconcile::Allowances::default())
            .await
            .unwrap()
            .all_passed
    );

    // Legacy DB grows: new payment after the importer ran.
    seed_payment(
        &env.legacy,
        MINER_B,
        999_999,
        VALID_HASH_B,
        ts("2025-09-02 10:00:00"),
    )
    .await;

    // Reconcile MUST now fail; specifically on the payments amount aggregate.
    let report = reconcile::run(&env.legacy, &env.target, &reconcile::Allowances::default())
        .await
        .unwrap();
    assert!(
        !report.all_passed,
        "reconcile should detect legacy growth after import"
    );
    let payments_check = report
        .checks
        .iter()
        .find(|c| c.name == "payments.amount_total_sompi")
        .unwrap();
    assert!(!payments_check.passed);
    assert_eq!(payments_check.legacy - payments_check.new, 999_999);
}
