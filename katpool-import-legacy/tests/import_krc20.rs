//! End-to-end importer test for the `pending_krc20_transfers`
//! transform.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use chrono::NaiveDateTime;
use sqlx::Row;

use katpool_import_legacy::transform::krc20;

mod common;
use common::{MINER_A, MINER_B, setup};

fn ts(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn seed_transfer(
    legacy: &sqlx::PgPool,
    first_txn_id: &str,
    sompi_to_miner: i64,
    nacho_amount: i64,
    address: &str,
    p2sh_address: &str,
    status: &str,
    timestamp: Option<NaiveDateTime>,
) {
    sqlx::query(
        "INSERT INTO pending_krc20_transfers
            (first_txn_id, sompi_to_miner, nacho_amount, address, p2sh_address,
             nacho_transfer_status, db_entry_status, timestamp)
         VALUES ($1, $2, $3, $4, $5, $6::status_enum, 'PENDING', $7)",
    )
    .bind(first_txn_id)
    .bind(sompi_to_miner)
    .bind(nacho_amount)
    .bind(address)
    .bind(p2sh_address)
    .bind(status)
    .bind(timestamp)
    .execute(legacy)
    .await
    .expect("seed pending_krc20_transfers row");
}

#[tokio::test]
async fn krc20_maps_each_status_correctly() {
    let env = setup().await;
    seed_transfer(
        &env.legacy,
        "tx-pending-1",
        100_000,
        50,
        MINER_A,
        "p2sh:abc",
        "PENDING",
        Some(ts("2025-09-01 10:00:00")),
    )
    .await;
    seed_transfer(
        &env.legacy,
        "tx-completed-1",
        200_000,
        100,
        MINER_B,
        "p2sh:def",
        "COMPLETED",
        Some(ts("2025-09-01 10:00:00")),
    )
    .await;
    seed_transfer(
        &env.legacy,
        "tx-failed-1",
        50_000,
        25,
        MINER_A,
        "p2sh:ghi",
        "FAILED",
        Some(ts("2025-09-01 10:00:00")),
    )
    .await;

    let stats = krc20::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.read, 3);
    assert_eq!(stats.inserted, 3);
    assert_eq!(stats.rejected, 0);

    // One cycle per row.
    let cycle_count: i64 = sqlx::query_scalar("SELECT count(*) FROM payout_cycle")
        .fetch_one(&env.target)
        .await
        .unwrap();
    assert_eq!(cycle_count, 3);

    // Per-status row count.
    for (legacy_status, new_status) in [
        ("PENDING", "pending"),
        ("COMPLETED", "completed"),
        ("FAILED", "failed"),
    ] {
        let n: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM krc20_pending_transfer WHERE status = '{new_status}'"
        ))
        .fetch_one(&env.target)
        .await
        .unwrap();
        assert_eq!(n, 1, "expected one {legacy_status} -> {new_status} row");
    }

    // FAILED cycle ends in `failed`; COMPLETED in `settled`;
    // PENDING in `planned`.
    let pending_cycle_status: String = sqlx::query_scalar(
        "SELECT c.status::text FROM payout_cycle c WHERE c.idempotency_key = 'krc20-legacy-pending-tx-pending-1'",
    )
    .fetch_one(&env.target)
    .await
    .unwrap();
    assert_eq!(pending_cycle_status, "planned");

    let completed_cycle_status: String = sqlx::query_scalar(
        "SELECT c.status::text FROM payout_cycle c WHERE c.idempotency_key = 'krc20-legacy-pending-tx-completed-1'",
    )
    .fetch_one(&env.target)
    .await
    .unwrap();
    assert_eq!(completed_cycle_status, "settled");

    let failed_cycle_status: String = sqlx::query_scalar(
        "SELECT c.status::text FROM payout_cycle c WHERE c.idempotency_key = 'krc20-legacy-pending-tx-failed-1'",
    )
    .fetch_one(&env.target)
    .await
    .unwrap();
    assert_eq!(failed_cycle_status, "failed");

    // FAILED payout has failure_reason set.
    let row = sqlx::query(
        "SELECT p.status::text, p.failure_reason FROM payout p
           JOIN payout_cycle c ON c.id = p.cycle_id
          WHERE c.idempotency_key = 'krc20-legacy-pending-tx-failed-1'",
    )
    .fetch_one(&env.target)
    .await
    .unwrap();
    assert_eq!(row.get::<String, _>("status"), "failed");
    assert!(
        row.get::<Option<String>, _>("failure_reason")
            .unwrap()
            .contains("legacy")
    );
}

#[tokio::test]
async fn krc20_idempotent_rerun() {
    let env = setup().await;
    seed_transfer(
        &env.legacy,
        "tx-pending-1",
        100_000,
        50,
        MINER_A,
        "p2sh:abc",
        "PENDING",
        Some(ts("2025-09-01 10:00:00")),
    )
    .await;

    let _ = krc20::run(&env.legacy, &env.target, false)
        .await
        .expect("run1");
    let stats2 = krc20::run(&env.legacy, &env.target, false)
        .await
        .expect("run2");
    assert_eq!(stats2.skipped, 1);
    assert_eq!(stats2.inserted, 0);

    let transfer_count: i64 = sqlx::query_scalar("SELECT count(*) FROM krc20_pending_transfer")
        .fetch_one(&env.target)
        .await
        .unwrap();
    assert_eq!(transfer_count, 1);
}

#[tokio::test]
async fn krc20_rejects_invalid_recipient_or_amounts() {
    let env = setup().await;
    seed_transfer(
        &env.legacy,
        "tx-bad-addr-1",
        1000,
        10,
        "not-a-kaspa-address",
        "p2sh:x",
        "PENDING",
        None,
    )
    .await;
    seed_transfer(
        &env.legacy,
        "tx-zero-sompi-1",
        0,
        10,
        MINER_A,
        "p2sh:x",
        "PENDING",
        None,
    )
    .await;
    seed_transfer(
        &env.legacy,
        "tx-zero-nacho-1",
        1000,
        0,
        MINER_A,
        "p2sh:x",
        "PENDING",
        None,
    )
    .await;

    let stats = krc20::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.read, 3);
    assert_eq!(stats.rejected, 3);
    assert_eq!(stats.inserted, 0);
}

#[tokio::test]
async fn krc20_dry_run_writes_nothing() {
    let env = setup().await;
    seed_transfer(
        &env.legacy,
        "tx-pending-1",
        100_000,
        50,
        MINER_A,
        "p2sh:abc",
        "PENDING",
        Some(ts("2025-09-01 10:00:00")),
    )
    .await;

    let stats = krc20::run(&env.legacy, &env.target, true)
        .await
        .expect("dry");
    assert_eq!(stats.inserted, 1);

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM krc20_pending_transfer")
        .fetch_one(&env.target)
        .await
        .unwrap();
    assert_eq!(n, 0);
}
