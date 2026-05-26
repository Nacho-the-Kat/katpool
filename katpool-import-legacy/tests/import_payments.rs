//! End-to-end importer test for the `payments` (KAS) and
//! `nacho_payments` (KRC-20) transforms.
//!
//! Both transforms share the same shape (group-by `transaction_hash`,
//! synthetic cycle, per-recipient payout), so they're covered in
//! one file: any cross-transform invariant that the schema
//! enforces (UNIQUE on `cycle_id, wallet_id`, FK to wallet, etc.)
//! is exercised once per kind.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use chrono::NaiveDateTime;
use katpool_db::repo::payout::{PayoutCycleStatus, PayoutKind};
use katpool_domain::WalletAddress;
use sqlx::Row;

use katpool_import_legacy::transform::{nacho_payments, payments};

mod common;
use common::{MINER_A, MINER_B, VALID_HASH_A, VALID_HASH_B, setup};

fn ts(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

async fn seed_payment(
    legacy: &sqlx::PgPool,
    wallet_addresses: &[&str],
    amount: i64,
    timestamp: Option<NaiveDateTime>,
    transaction_hash: &str,
) {
    let owned: Vec<String> = wallet_addresses.iter().map(|s| (*s).to_owned()).collect();
    sqlx::query(
        "INSERT INTO payments (wallet_address, amount, timestamp, transaction_hash)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(&owned)
    .bind(amount)
    .bind(timestamp)
    .bind(transaction_hash)
    .execute(legacy)
    .await
    .expect("seed payments row");
}

async fn seed_nacho_payment(
    legacy: &sqlx::PgPool,
    wallet_addresses: &[&str],
    nacho_amount: i64,
    timestamp: Option<NaiveDateTime>,
    transaction_hash: &str,
) {
    let owned: Vec<String> = wallet_addresses.iter().map(|s| (*s).to_owned()).collect();
    sqlx::query(
        "INSERT INTO nacho_payments (wallet_address, nacho_amount, timestamp, transaction_hash)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(&owned)
    .bind(nacho_amount)
    .bind(timestamp)
    .bind(transaction_hash)
    .execute(legacy)
    .await
    .expect("seed nacho_payments row");
}

#[tokio::test]
async fn payments_groups_by_tx_hash() {
    let env = setup().await;
    seed_payment(
        &env.legacy,
        &[MINER_A],
        1000,
        Some(ts("2025-09-01 10:00:00")),
        VALID_HASH_A,
    )
    .await;
    seed_payment(
        &env.legacy,
        &[MINER_B],
        2000,
        Some(ts("2025-09-01 10:00:01")),
        VALID_HASH_A,
    )
    .await;
    seed_payment(
        &env.legacy,
        &[MINER_A],
        3000,
        Some(ts("2025-09-01 11:00:00")),
        VALID_HASH_B,
    )
    .await;

    let stats = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.read, 3);
    assert_eq!(stats.inserted, 3);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.rejected, 0);

    // Two cycles created.
    let cycle_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payout_cycle WHERE kind = 'kas' AND idempotency_key LIKE 'kas-legacy-%'")
            .fetch_one(&env.target)
            .await
            .unwrap();
    assert_eq!(cycle_count, 2);

    // The 2-recipient cycle (hash A) has 2 payouts; the 1-recipient cycle (hash B) has 1.
    let cycle_a_payouts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM payout p
           JOIN payout_cycle c ON c.id = p.cycle_id
          WHERE c.idempotency_key = $1",
    )
    .bind(format!("kas-legacy-{VALID_HASH_A}"))
    .fetch_one(&env.target)
    .await
    .unwrap();
    assert_eq!(cycle_a_payouts, 2);

    // Cycle totals are set + cycle marked settled.
    let row = sqlx::query("SELECT total_sompi, total_recipients, status::text FROM payout_cycle WHERE idempotency_key = $1")
        .bind(format!("kas-legacy-{VALID_HASH_A}"))
        .fetch_one(&env.target)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("total_sompi"), 3000);
    assert_eq!(row.get::<i32, _>("total_recipients"), 2);
    assert_eq!(row.get::<String, _>("status"), "settled");
}

#[tokio::test]
async fn payments_idempotent_on_rerun() {
    let env = setup().await;
    seed_payment(
        &env.legacy,
        &[MINER_A],
        5000,
        Some(ts("2025-09-01 10:00:00")),
        VALID_HASH_A,
    )
    .await;

    let _ = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("run1");
    let stats2 = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("run2");

    // Second run: every payout already exists.
    assert_eq!(stats2.read, 1);
    assert_eq!(stats2.inserted, 0);
    assert_eq!(stats2.skipped, 1);

    // Still exactly one cycle + one payout.
    let cycle_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM payout_cycle WHERE idempotency_key = $1")
            .bind(format!("kas-legacy-{VALID_HASH_A}"))
            .fetch_one(&env.target)
            .await
            .unwrap();
    assert_eq!(cycle_count, 1);

    let payout_count: i64 = sqlx::query_scalar("SELECT count(*) FROM payout")
        .fetch_one(&env.target)
        .await
        .unwrap();
    assert_eq!(payout_count, 1);
}

#[tokio::test]
async fn payments_rejects_invalid_tx_hash() {
    let env = setup().await;
    seed_payment(
        &env.legacy,
        &[MINER_A],
        1000,
        Some(ts("2025-09-01 10:00:00")),
        "not-a-hex-hash",
    )
    .await;
    seed_payment(
        &env.legacy,
        &[MINER_A],
        2000,
        Some(ts("2025-09-01 10:00:00")),
        "deadbeef",
    )
    .await;

    let stats = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.read, 2);
    assert_eq!(stats.rejected, 2);
    assert_eq!(stats.inserted, 0);
}

#[tokio::test]
async fn payments_rejects_invalid_recipient_wallet() {
    let env = setup().await;
    seed_payment(
        &env.legacy,
        &["not-a-kaspa-address"],
        1000,
        Some(ts("2025-09-01 10:00:00")),
        VALID_HASH_A,
    )
    .await;

    let stats = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.read, 1);
    assert_eq!(stats.rejected, 1);

    // The cycle was still created (tx_hash was valid); it just has
    // zero payouts under it. That's fine — the reconcile step
    // catches any sum mismatch.
    let row = sqlx::query(
        "SELECT total_sompi, total_recipients FROM payout_cycle WHERE idempotency_key = $1",
    )
    .bind(format!("kas-legacy-{VALID_HASH_A}"))
    .fetch_one(&env.target)
    .await
    .unwrap();
    assert_eq!(row.get::<i64, _>("total_sompi"), 0);
    assert_eq!(row.get::<i32, _>("total_recipients"), 0);
}

#[tokio::test]
async fn payments_dry_run_writes_nothing() {
    let env = setup().await;
    seed_payment(
        &env.legacy,
        &[MINER_A],
        1000,
        Some(ts("2025-09-01 10:00:00")),
        VALID_HASH_A,
    )
    .await;

    let stats = payments::run(&env.legacy, &env.target, true)
        .await
        .expect("dry_run");
    assert_eq!(stats.read, 1);
    assert_eq!(stats.inserted, 1);

    let cycle_count: i64 = sqlx::query_scalar("SELECT count(*) FROM payout_cycle")
        .fetch_one(&env.target)
        .await
        .unwrap();
    assert_eq!(cycle_count, 0);
}

#[tokio::test]
async fn nacho_payments_inserts_correct_kind_and_hashes() {
    let env = setup().await;
    seed_nacho_payment(
        &env.legacy,
        &[MINER_A],
        500_000,
        Some(ts("2025-09-01 10:00:00")),
        VALID_HASH_A,
    )
    .await;

    let stats = nacho_payments::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.inserted, 1);

    // Cycle kind = krc20_nacho.
    let kind: String =
        sqlx::query_scalar("SELECT kind::text FROM payout_cycle WHERE idempotency_key = $1")
            .bind(format!("krc20-legacy-{VALID_HASH_A}"))
            .fetch_one(&env.target)
            .await
            .unwrap();
    assert_eq!(kind, "krc20_nacho");
    assert_eq!(PayoutKind::Krc20Nacho as u8, PayoutKind::Krc20Nacho as u8); // keep enum visible
    assert!(matches!(
        PayoutCycleStatus::Settled,
        PayoutCycleStatus::Settled
    ));

    // Payout has BOTH commit and reveal hashes filled.
    let row = sqlx::query(
        "SELECT krc20_commit_hash, krc20_reveal_hash, tx_hash
           FROM payout p JOIN payout_cycle c ON c.id = p.cycle_id
          WHERE c.idempotency_key = $1",
    )
    .bind(format!("krc20-legacy-{VALID_HASH_A}"))
    .fetch_one(&env.target)
    .await
    .unwrap();
    assert!(row.try_get::<Vec<u8>, _>("krc20_commit_hash").is_ok());
    assert!(row.try_get::<Vec<u8>, _>("krc20_reveal_hash").is_ok());
    // KAS tx_hash NOT set for nacho payouts.
    assert!(
        row.try_get::<Option<Vec<u8>>, _>("tx_hash")
            .unwrap()
            .is_none()
    );

    // Wallet was ensured.
    let addr = WalletAddress::new(MINER_A.to_owned()).unwrap();
    let w = katpool_db::repo::wallet::find_by_address(&env.target, &addr)
        .await
        .unwrap();
    assert!(w.is_some());
}
