//! Integration tests for KAS cycle planning (M4.3).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use std::time::Duration;

use katpool_db::repo::payout::{self, PayoutKind, PayoutStatus};
use katpool_db::repo::{block, share_allocation, wallet, worker};
use katpool_db::{PoolConfig, build_pool, migrate};
use katpool_domain::{BlockHash, CorrelationId, DaaScore, WalletAddress, WorkerName};
use payout_kas::{DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI, PlanKasCycleParams, plan_kas_cycle};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn fresh_pool() -> (sqlx::PgPool, ContainerAsync<Postgres>) {
    let container = Postgres::default().start().await.expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    let cfg = PoolConfig {
        url,
        min_connections: 1,
        max_connections: 4,
        acquire_timeout: Duration::from_secs(10),
        idle_timeout: Duration::from_secs(60),
        max_lifetime: Duration::from_secs(300),
        statement_timeout: Duration::from_secs(30),
        application_name: "payout-kas-plan-test".to_owned(),
    };

    let pool = build_pool(&cfg).await.expect("pool");
    migrate::run(&pool).await.expect("migrate");
    (pool, container)
}

fn sample_wallet_addr() -> WalletAddress {
    WalletAddress::new("kaspa:qz4j8mu269z8llgcczmfukm9fan2fq822kzxu4cfukd5fqrhxpsv2zhs9jxnp")
        .expect("valid")
}

fn second_wallet_addr() -> WalletAddress {
    WalletAddress::new("kaspa:qrxd24c5w6pl2qa9k7q5e0lyepuu4r5t2f6awvxllk0a83qqfys9rhxpsv2zhs9j")
        .expect("valid")
}

fn sample_worker_name() -> WorkerName {
    WorkerName::new("rig-01").expect("valid")
}

async fn seed_two_wallet_allocations(
    pool: &sqlx::PgPool,
) -> (
    katpool_db::repo::BlockId,
    katpool_db::repo::WalletId,
    katpool_db::repo::WalletId,
) {
    let w1 = wallet::ensure(pool, &sample_wallet_addr(), "mainnet")
        .await
        .expect("wallet 1");
    let wk = worker::ensure(pool, w1.id, &sample_worker_name())
        .await
        .expect("worker");
    let w2 = wallet::ensure(pool, &second_wallet_addr(), "mainnet")
        .await
        .expect("wallet 2");

    let hash = BlockHash::from_bytes([9_u8; 32]);
    let block_id = block::insert(
        pool,
        hash,
        w1.id,
        wk.id,
        DaaScore::new(1),
        0,
        CorrelationId::new_v4(),
    )
    .await
    .expect("block");
    block::mark_submitted(pool, hash).await.expect("submit");
    block::mark_confirmed_blue(pool, hash, 1)
        .await
        .expect("confirm");
    block::mark_matured(pool, hash, 5_000_000_000)
        .await
        .expect("mature");

    let rows = vec![
        share_allocation::NewAllocation {
            wallet_id: w1.id,
            weight: 60.0,
            window_total: 100.0,
            gross_share_sompi: 3_000_000_000,
            pool_fee_sompi: 15_075_000,
            nacho_accrual_sompi: 7_425_000,
            net_payout_sompi: 2_977_500_000,
            applied_topline_bps: 75,
            applied_rebate_bps: 3_300,
            applied_tier: share_allocation::DbWalletTier::Standard,
        },
        share_allocation::NewAllocation {
            wallet_id: w2.id,
            weight: 40.0,
            window_total: 100.0,
            gross_share_sompi: 2_000_000_000,
            pool_fee_sompi: 10_050_000,
            nacho_accrual_sompi: 4_950_000,
            net_payout_sompi: 1_985_000_000,
            applied_topline_bps: 75,
            applied_rebate_bps: 3_300,
            applied_tier: share_allocation::DbWalletTier::Standard,
        },
    ];
    share_allocation::insert_batch(pool, block_id, &rows)
        .await
        .expect("allocations");

    (block_id, w1.id, w2.id)
}

#[tokio::test]
async fn kas_eligible_wallets_respects_threshold_and_confirmed_paid() {
    let (pool, _ctr) = fresh_pool().await;
    let (_block_id, w1, w2) = seed_two_wallet_allocations(&pool).await;

    let eligible = payout::list_kas_eligible_wallets(&pool, DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI)
        .await
        .expect("eligible");
    assert_eq!(eligible.len(), 2);
    assert_eq!(eligible[0].wallet_id, w1);
    assert_eq!(eligible[0].payable_sompi, 2_977_500_000);
    assert_eq!(eligible[1].wallet_id, w2);
    assert_eq!(eligible[1].payable_sompi, 1_985_000_000);

    let cycle = payout::create_cycle(&pool, PayoutKind::Kas, DaaScore::new(1), DaaScore::new(2))
        .await
        .expect("cycle");
    let p1 = payout::insert_payout(&pool, cycle.id, w1, 1_000_000_000)
        .await
        .expect("partial payout");
    payout::mark_payout_submitted(&pool, p1.id, BlockHash::from_bytes([11_u8; 32]))
        .await
        .expect("submit");
    payout::mark_payout_confirmed(&pool, p1.id)
        .await
        .expect("confirm");

    let after = payout::list_kas_eligible_wallets(&pool, DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI)
        .await
        .expect("eligible after partial pay");
    assert_eq!(after.len(), 2);
    let w1_row = after.iter().find(|r| r.wallet_id == w1).expect("w1");
    assert_eq!(w1_row.confirmed_paid_sompi, 1_000_000_000);
    assert_eq!(w1_row.payable_sompi, 2_977_500_000 - 1_000_000_000);

    let high_bar = payout::list_kas_eligible_wallets(&pool, 3_000_000_000)
        .await
        .expect("high threshold");
    assert!(
        high_bar.is_empty(),
        "w1 payable is 1_977_500_000 after partial confirm — below 3 KAS bar"
    );

    let mid_bar = payout::list_kas_eligible_wallets(&pool, 1_980_000_000)
        .await
        .expect("mid threshold");
    assert_eq!(mid_bar.len(), 1);
    assert_eq!(mid_bar[0].wallet_id, w2);
}

#[tokio::test]
async fn plan_kas_cycle_is_idempotent_and_sets_totals() {
    let (pool, _ctr) = fresh_pool().await;
    let (_block_id, w1, _w2) = seed_two_wallet_allocations(&pool).await;

    let params = PlanKasCycleParams {
        daa_start: DaaScore::new(1_000),
        daa_end: DaaScore::new(2_000),
        threshold_sompi: DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI,
    };

    let first = plan_kas_cycle(&pool, params).await.expect("plan");
    assert_eq!(first.cycle.idempotency_key, "kas-1000-2000");
    assert_eq!(first.cycle.total_recipients, 2);
    assert_eq!(
        first.cycle.total_sompi,
        2_977_500_000_i64 + 1_985_000_000_i64
    );
    assert_eq!(first.payouts.len(), 2);
    assert!(
        first
            .payouts
            .iter()
            .all(|p| p.status == PayoutStatus::Planned)
    );

    let second = plan_kas_cycle(&pool, params).await.expect("replay");
    assert_eq!(second.cycle.id, first.cycle.id);
    assert_eq!(second.payouts.len(), 2);

    let w1_payout = second
        .payouts
        .iter()
        .find(|p| p.wallet_id == w1)
        .expect("w1 payout");
    assert_eq!(w1_payout.amount_sompi, 2_977_500_000);
}

#[tokio::test]
async fn plan_kas_cycle_excludes_wallet_below_threshold() {
    let (pool, _ctr) = fresh_pool().await;
    let (_block_id, w1, _w2) = seed_two_wallet_allocations(&pool).await;

    let params = PlanKasCycleParams {
        daa_start: DaaScore::new(10),
        daa_end: DaaScore::new(20),
        threshold_sompi: 2_900_000_000,
    };
    let result = plan_kas_cycle(&pool, params).await.expect("plan");
    assert_eq!(result.cycle.total_recipients, 1);
    assert_eq!(result.payouts.len(), 1);
    assert_eq!(result.payouts[0].wallet_id, w1);
}
