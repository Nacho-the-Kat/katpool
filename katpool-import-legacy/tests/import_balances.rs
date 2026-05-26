//! End-to-end importer test for the `miners_balance` transform.
//!
//! Exercises:
//! - the deterministic `set_accrual` semantics (re-running the
//!   importer never accumulates),
//! - rejection of malformed or negative rebate values,
//! - skipping of zero-rebate rows,
//! - cross-transform interaction (wallet rows created here are
//!   visible to subsequent transforms, but we only test the
//!   wallet-creation side-effect here, not the cross-transform
//!   reconciliation — that lives in `import_reconcile.rs`).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use katpool_db::repo::{nacho_rebate, wallet};
use katpool_domain::WalletAddress;
use sqlx::types::BigDecimal;
use std::str::FromStr;

use katpool_import_legacy::transform::balances;

mod common;
use common::{MINER_A, MINER_B, setup};

async fn seed_balance(
    legacy: &sqlx::PgPool,
    id: &str,
    miner_id: Option<&str>,
    wallet: Option<&str>,
    balance: Option<BigDecimal>,
    nacho_rebate_kas: Option<BigDecimal>,
) {
    sqlx::query(
        "INSERT INTO miners_balance (id, miner_id, wallet, balance, nacho_rebate_kas)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(miner_id)
    .bind(wallet)
    .bind(balance)
    .bind(nacho_rebate_kas)
    .execute(legacy)
    .await
    .expect("seed miners_balance row");
}

#[tokio::test]
async fn balances_imports_then_idempotent() {
    let env = setup().await;
    seed_balance(
        &env.legacy,
        "wallet-A.JANKS5",
        Some("JANKS5Pro"),
        Some(MINER_A),
        Some(BigDecimal::from_str("1000000").unwrap()),
        Some(BigDecimal::from_str("12345").unwrap()),
    )
    .await;
    seed_balance(
        &env.legacy,
        "wallet-B.KS5P02",
        Some("KS5P02"),
        Some(MINER_B),
        Some(BigDecimal::from_str("500000").unwrap()),
        Some(BigDecimal::from_str("6789").unwrap()),
    )
    .await;

    let stats = balances::run(&env.legacy, &env.target, false)
        .await
        .expect("run 1");
    assert_eq!(stats.read, 2);
    assert_eq!(stats.inserted, 2);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.rejected, 0);

    // Verify the actual stored values.
    let addr_a = WalletAddress::new(MINER_A.to_owned()).unwrap();
    let w_a = wallet::find_by_address(&env.target, &addr_a)
        .await
        .unwrap()
        .unwrap();
    let r_a = nacho_rebate::get(&env.target, w_a.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(r_a.accrued_sompi, 12345);
    assert_eq!(r_a.paid_sompi, 0);

    // Re-run: identical stats, identical values. Idempotency.
    let stats2 = balances::run(&env.legacy, &env.target, false)
        .await
        .expect("run 2");
    assert_eq!(stats2.inserted, 2);
    let r_a2 = nacho_rebate::get(&env.target, w_a.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        r_a2.accrued_sompi, 12345,
        "set-semantics must not double-accrue"
    );
}

#[tokio::test]
async fn balances_skips_zero_and_null_rebate() {
    let env = setup().await;
    seed_balance(
        &env.legacy,
        "wallet-zero",
        Some("JANKS5Pro"),
        Some(MINER_A),
        None,
        Some(BigDecimal::from_str("0").unwrap()),
    )
    .await;
    seed_balance(
        &env.legacy,
        "wallet-null",
        Some("KS5P02"),
        Some(MINER_B),
        None,
        None,
    )
    .await;

    let stats = balances::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.read, 2);
    assert_eq!(stats.inserted, 0);
    assert_eq!(stats.skipped, 2);
    assert_eq!(stats.rejected, 0);

    // Wallet rows ARE created even for zero-rebate (they may be
    // referenced by other transforms). The rebate row is NOT.
    let addr_a = WalletAddress::new(MINER_A.to_owned()).unwrap();
    let w_a = wallet::find_by_address(&env.target, &addr_a)
        .await
        .unwrap()
        .unwrap();
    let r_a = nacho_rebate::get(&env.target, w_a.id).await.unwrap();
    assert!(
        r_a.is_none(),
        "zero rebate must not insert nacho_rebate_accrual row"
    );
}

#[tokio::test]
async fn balances_rejects_invalid_wallet_and_negative() {
    let env = setup().await;
    seed_balance(
        &env.legacy,
        "bad-wallet",
        Some("JANKS5Pro"),
        Some("not-a-kaspa-address"),
        None,
        Some(BigDecimal::from_str("10").unwrap()),
    )
    .await;
    seed_balance(
        &env.legacy,
        "missing-wallet",
        Some("KS5P02"),
        None,
        None,
        Some(BigDecimal::from_str("10").unwrap()),
    )
    .await;
    seed_balance(
        &env.legacy,
        "negative-rebate",
        Some("KS5P02"),
        Some(MINER_A),
        None,
        Some(BigDecimal::from_str("-1").unwrap()),
    )
    .await;

    let stats = balances::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.read, 3);
    assert_eq!(stats.inserted, 0);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.rejected, 3);
}

#[tokio::test]
async fn balances_dry_run_writes_nothing() {
    let env = setup().await;
    seed_balance(
        &env.legacy,
        "wallet-A.JANKS5",
        Some("JANKS5Pro"),
        Some(MINER_A),
        None,
        Some(BigDecimal::from_str("12345").unwrap()),
    )
    .await;

    let stats = balances::run(&env.legacy, &env.target, true)
        .await
        .expect("dry run");
    assert_eq!(stats.read, 1);
    assert_eq!(stats.inserted, 1);

    let addr_a = WalletAddress::new(MINER_A.to_owned()).unwrap();
    let found = wallet::find_by_address(&env.target, &addr_a).await.unwrap();
    assert!(
        found.is_none(),
        "dry_run must not create wallet rows in target"
    );
}
