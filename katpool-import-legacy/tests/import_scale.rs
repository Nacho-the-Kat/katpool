//! Scale acceptance test for the legacy importer.
//!
//! Seeds the legacy DB with a synthetic dataset two orders of
//! magnitude larger than the per-transform happy-path tests, then
//! runs the full importer + reconcile pass. Produces empirical
//! evidence that:
//!
//! 1. The importer scales linearly enough to finish a production
//!    cutover inside the 30-minute window described in
//!    `docs/runbooks/14-legacy-importer.md`.
//! 2. The reconcile pass converges on a non-trivial dataset.
//! 3. Per-block FK chains (`wallet` ← `worker` ← `block`) hold up
//!    when many wallets share many workers across many blocks.
//!
//! ## Calibration
//!
//! Production scale (`docs/db-schema.md`'s legacy reference
//! capture, May 2026):
//!
//! | Table                    | Rows    |
//! |--------------------------|---------|
//! | `block_details`          | 539,397 |
//! | `miners_balance`         |   2,623 |
//! | `payments`               |  ~30K   |
//! | `nacho_payments`         |  ~12K   |
//! | `pending_krc20_transfers`|   ~5K   |
//!
//! Running production scale in CI is wasteful. Two test entry
//! points: a CI-default 1,000-block run (~10s), and a longer
//! `#[ignore]`-able 10,000-block run for local rehearsal.

// `eprintln!` is denied workspace-wide as an anti-anti-pattern
// in production code, but scale tests dump timing telemetry that
// makes regressions immediately legible in CI logs; we relax the
// gate here only. Same rationale for `integer_division` (block
// counts), `format_collect` (hex synth), and
// `cognitive_complexity` (linear orchestration over 5
// transforms).
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::print_stderr,
    clippy::integer_division,
    clippy::cognitive_complexity,
    clippy::format_collect
)]

use std::time::Instant;

use chrono::NaiveDateTime;
use sqlx::types::BigDecimal;

use katpool_import_legacy::reconcile;
use katpool_import_legacy::transform::{balances, blocks, krc20, nacho_payments, payments};

mod common;
use common::setup;

const WALLETS: &[&str] = &[
    "kaspa:qypczcz0lhyf3tfsuqj86e7qc8us7r8a53nhlr4u6x4kq38td0hsjycf7sya7zq",
    "kaspa:qzncghl8re9h35hp6n5wyxtslhevj6462qkrkqzlfkrs2mpkfkc5xe9s3tga7",
    "kaspa:qz4j8mu269z8llgcczmfukm9fan2fq822kzxu4cfukd5fqrhxpsv2zhs9jxnp",
];

const POOL_ADDR: &str = "kaspa:qz4j8mu269z8llgcczmfukm9fan2fq822kzxu4cfukd5fqrhxpsv2zhs9jxnp";

fn synth_block_hash(i: usize) -> String {
    // Deterministic + collision-free across our test range: pack
    // `i` into the low bytes of a 32-byte buffer and prepend a
    // recognisable marker so the synthetic hashes don't look like
    // real hashes if they leak into a log.
    let mut bytes = [0u8; 32];
    bytes[0..8].copy_from_slice(b"katpool!");
    bytes[24..32].copy_from_slice(&(i as u64).to_be_bytes());
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn ts(seed: usize) -> NaiveDateTime {
    NaiveDateTime::parse_from_str("2025-09-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
        + chrono::Duration::seconds(seed as i64)
}

async fn seed_blocks(legacy: &sqlx::PgPool, count: usize) {
    const CHUNK: usize = 500;
    for chunk_start in (0..count).step_by(CHUNK) {
        let chunk_end = (chunk_start + CHUNK).min(count);
        let mut q: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(
            "INSERT INTO block_details \
             (mined_block_hash, miner_id, pool_address, wallet, daa_score, miner_reward, timestamp) ",
        );
        q.push_values(chunk_start..chunk_end, |mut b, i| {
            let wallet = WALLETS[i % WALLETS.len()];
            let worker = format!("worker-{}", i % 25);
            b.push_bind(synth_block_hash(i))
                .push_bind(worker)
                .push_bind(POOL_ADDR)
                .push_bind(wallet)
                .push_bind((400_000_000_i64 + i as i64).to_string())
                .push_bind(275_000_000_i64)
                .push_bind(ts(i));
        });
        q.build()
            .execute(legacy)
            .await
            .expect("seed block_details chunk");
    }
}

async fn seed_balances(legacy: &sqlx::PgPool) {
    for (i, w) in WALLETS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO miners_balance (id, miner_id, wallet, balance, nacho_rebate_kas)
             VALUES ($1, $2, $3, NULL, $4)",
        )
        .bind(format!("miners_balance-{i}"))
        .bind(format!("worker-{i}"))
        .bind(*w)
        .bind(BigDecimal::from(10_000 + i as i64 * 100))
        .execute(legacy)
        .await
        .expect("seed miners_balance");
    }
}

async fn seed_payments(legacy: &sqlx::PgPool, cycles: usize, per_cycle: usize) {
    for cycle in 0..cycles {
        let tx_hash = synth_block_hash(1_000_000 + cycle);
        for r in 0..per_cycle {
            let wallet = WALLETS[(cycle + r) % WALLETS.len()];
            sqlx::query(
                "INSERT INTO payments (wallet_address, amount, timestamp, transaction_hash)
                 VALUES (ARRAY[$1], $2, $3, $4)",
            )
            .bind(wallet)
            .bind(10_000_i64 + (cycle as i64 * 100) + r as i64)
            .bind(ts(cycle * per_cycle + r))
            .bind(&tx_hash)
            .execute(legacy)
            .await
            .expect("seed payments");
        }
    }
}

async fn seed_nacho_payments(legacy: &sqlx::PgPool, cycles: usize, per_cycle: usize) {
    for cycle in 0..cycles {
        let tx_hash = synth_block_hash(2_000_000 + cycle);
        for r in 0..per_cycle {
            let wallet = WALLETS[(cycle + r) % WALLETS.len()];
            sqlx::query(
                "INSERT INTO nacho_payments (wallet_address, nacho_amount, timestamp, transaction_hash)
                 VALUES (ARRAY[$1], $2, $3, $4)",
            )
            .bind(wallet)
            .bind(50_i64 + (cycle as i64) + r as i64)
            .bind(ts(cycle * per_cycle + r))
            .bind(&tx_hash)
            .execute(legacy)
            .await
            .expect("seed nacho_payments");
        }
    }
}

async fn seed_krc20(legacy: &sqlx::PgPool, count: usize) {
    for i in 0..count {
        let status = match i % 3 {
            0 => "PENDING",
            1 => "COMPLETED",
            _ => "FAILED",
        };
        sqlx::query(
            "INSERT INTO pending_krc20_transfers
                (first_txn_id, sompi_to_miner, nacho_amount, address, p2sh_address,
                 nacho_transfer_status, db_entry_status, timestamp)
             VALUES ($1, $2, $3, $4, $5, $6::status_enum, 'PENDING', $7)",
        )
        .bind(format!("legacy-krc20-{i}"))
        .bind(1000_i64 + i as i64)
        .bind(10_i64 + (i as i64 / 3))
        .bind(WALLETS[i % WALLETS.len()])
        .bind(format!("p2sh:legacy-{i}"))
        .bind(status)
        .bind(ts(i))
        .execute(legacy)
        .await
        .expect("seed pending_krc20_transfers");
    }
}

/// Default CI run. ~1K blocks; finishes in ~10s on the runner.
#[tokio::test]
async fn scale_acceptance_ci_default() {
    run_scale(1_000).await;
}

/// Larger local-rehearsal run, marked `#[ignore]` so it doesn't
/// run by default but is available via `cargo test -- --ignored`.
#[tokio::test]
#[ignore = "scale test; ~30-60s; run with `cargo test -- --ignored`"]
async fn scale_acceptance_local_rehearsal() {
    run_scale(10_000).await;
}

#[allow(clippy::too_many_lines)]
async fn run_scale(block_count: usize) {
    let env = setup().await;

    let seed_start = Instant::now();
    seed_blocks(&env.legacy, block_count).await;
    seed_balances(&env.legacy).await;
    seed_payments(&env.legacy, block_count / 50, 3).await;
    seed_nacho_payments(&env.legacy, block_count / 100, 2).await;
    seed_krc20(&env.legacy, block_count / 20).await;
    let seed_elapsed = seed_start.elapsed();
    eprintln!(
        "scale: seeded blocks={} payments_cycles={} nacho_cycles={} krc20={} in {:.2?}",
        block_count,
        block_count / 50,
        block_count / 100,
        block_count / 20,
        seed_elapsed
    );

    let import_start = Instant::now();
    let blocks_stats = blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("blocks");
    let balances_stats = balances::run(&env.legacy, &env.target, false)
        .await
        .expect("balances");
    let payments_stats = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("payments");
    let nacho_stats = nacho_payments::run(&env.legacy, &env.target, false)
        .await
        .expect("nacho");
    let (krc20_stats, _) = krc20::run(&env.legacy, &env.target, false)
        .await
        .expect("krc20");
    let import_elapsed = import_start.elapsed();

    eprintln!(
        "scale: imported blocks={} balances={} payments={} nacho={} krc20={} in {:.2?}",
        blocks_stats.inserted,
        balances_stats.inserted,
        payments_stats.inserted,
        nacho_stats.inserted,
        krc20_stats.inserted,
        import_elapsed
    );

    let reconcile_start = Instant::now();
    let report = reconcile::run(
        &env.legacy,
        &env.target,
        &reconcile::Allowances::default(),
        &std::collections::HashSet::new(),
    )
    .await
    .expect("reconcile");
    let reconcile_elapsed = reconcile_start.elapsed();
    eprintln!(
        "scale: reconciled {} checks in {:.2?}",
        report.checks.len(),
        reconcile_elapsed
    );

    assert_eq!(blocks_stats.rejected, 0);
    assert_eq!(balances_stats.rejected, 0);
    assert_eq!(payments_stats.rejected, 0);
    assert_eq!(nacho_stats.rejected, 0);
    assert_eq!(krc20_stats.rejected, 0);

    for c in &report.checks {
        assert!(
            c.passed,
            "scale-test reconcile check `{}` mismatch: legacy={} new={}",
            c.name, c.legacy, c.new
        );
    }
    assert!(report.all_passed);

    // Throughput sentinel: catch regressions that would blow the
    // 30-minute cutover budget at production scale.
    let max_secs = if block_count >= 5_000 { 120 } else { 60 };
    assert!(
        import_elapsed.as_secs() < max_secs,
        "scale-test import took {import_elapsed:.2?}, expected < {max_secs}s for {block_count} blocks"
    );

    // Idempotent re-run: zero inserts, every prior row skipped.
    let blocks2 = blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("blocks rerun");
    assert_eq!(blocks2.inserted, 0);
    assert_eq!(blocks2.skipped, blocks_stats.inserted);

    let payments2 = payments::run(&env.legacy, &env.target, false)
        .await
        .expect("payments rerun");
    assert_eq!(payments2.inserted, 0);

    let (krc20_2, _) = krc20::run(&env.legacy, &env.target, false)
        .await
        .expect("krc20 rerun");
    assert_eq!(krc20_2.inserted, 0);

    let report2 = reconcile::run(
        &env.legacy,
        &env.target,
        &reconcile::Allowances::default(),
        &std::collections::HashSet::new(),
    )
    .await
    .expect("reconcile rerun");
    for c in &report2.checks {
        assert!(c.passed, "post-rerun reconcile mismatch on `{}`", c.name);
    }
}
