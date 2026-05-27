//! End-to-end tests for the accountant's event consumer.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use std::time::Duration;

use katpool_domain::{CorrelationId, PoolEvent, ShareRejectReason};
use tokio::sync::broadcast;
use tokio::time::sleep;

use accountant::{ConsumerConfig, EventConsumer};

mod common;
use common::{
    HASH_A, HASH_B, MINER_A, MINER_B, block_accepted, block_found, hash as h, setup,
    share_credited, wallet as w,
};

fn cfg() -> ConsumerConfig {
    ConsumerConfig::new("test".to_owned(), "mainnet".to_owned()).unwrap()
}

#[tokio::test]
async fn share_credited_persists_wallet_worker_share() {
    let env = setup().await;
    let consumer = EventConsumer::new(env.db.clone(), cfg());

    let event = share_credited(MINER_A, "rig-01", 2048.0, 1_000_000);
    let corr = match &event {
        PoolEvent::ShareCredited { correlation_id, .. } => *correlation_id,
        _ => panic!("unexpected variant"),
    };
    consumer.handle_event(event).await;

    // Wallet exists.
    let wallet_addr = w(MINER_A);
    let wallet_row = katpool_db::repo::wallet::find_by_address(&env.db, &wallet_addr)
        .await
        .unwrap()
        .expect("wallet row created");

    // Worker exists under that wallet.
    let workers = katpool_db::repo::worker::list_for_wallet(&env.db, wallet_row.id)
        .await
        .unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].name, "rig-01");

    // Share exists with the right correlation_id.
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM share WHERE wallet_id = $1 AND correlation_id = $2",
    )
    .bind(wallet_row.id.0)
    .bind(*corr.as_uuid())
    .fetch_one(&env.db)
    .await
    .unwrap();
    assert_eq!(count, 1, "share row created");
}

#[tokio::test]
async fn share_credited_aggregates_weight_across_events() {
    let env = setup().await;
    let consumer = EventConsumer::new(env.db.clone(), cfg());

    consumer
        .handle_event(share_credited(MINER_A, "rig-01", 1024.0, 1_000_000))
        .await;
    consumer
        .handle_event(share_credited(MINER_A, "rig-01", 2048.0, 1_000_001))
        .await;
    consumer
        .handle_event(share_credited(MINER_B, "rig-02", 512.0, 1_000_002))
        .await;

    let wallet_a = katpool_db::repo::wallet::find_by_address(&env.db, &w(MINER_A))
        .await
        .unwrap()
        .unwrap();
    let weight_a = katpool_db::repo::share::sum_weight_for_window(
        &env.db,
        wallet_a.id,
        katpool_domain::DaaScore::new(0),
        katpool_domain::DaaScore::new(u64::from(u32::MAX)),
    )
    .await
    .unwrap();
    assert!(
        (weight_a - 3072.0).abs() < 1e-9,
        "expected weight_a = 3072 (= 1024 + 2048); got {weight_a}"
    );

    let total = katpool_db::repo::share::total_weight_for_window(
        &env.db,
        katpool_domain::DaaScore::new(0),
        katpool_domain::DaaScore::new(u64::from(u32::MAX)),
    )
    .await
    .unwrap();
    assert!(
        (total - 3584.0).abs() < 1e-9,
        "expected total = 3584; got {total}"
    );
}

#[tokio::test]
async fn block_found_then_accepted_transitions_status() {
    let env = setup().await;
    let consumer = EventConsumer::new(env.db.clone(), cfg());

    let corr = CorrelationId::new_v4();
    consumer
        .handle_event(block_found(MINER_A, "rig-01", HASH_A, 1_000_000, corr))
        .await;
    consumer.handle_event(block_accepted(HASH_A, corr)).await;

    let block = katpool_db::repo::block::find_by_hash(&env.db, h(HASH_A))
        .await
        .unwrap()
        .expect("block row exists");
    assert_eq!(
        block.status,
        katpool_db::repo::block::BlockStatus::SubmittedToNode,
        "status should advance to submitted_to_node after BlockAccepted"
    );
    assert!(block.submitted_at.is_some());
}

#[tokio::test]
async fn duplicate_block_found_is_idempotent() {
    let env = setup().await;
    let consumer = EventConsumer::new(env.db.clone(), cfg());

    let corr = CorrelationId::new_v4();
    consumer
        .handle_event(block_found(MINER_A, "rig-01", HASH_A, 1_000_000, corr))
        .await;
    consumer
        .handle_event(block_found(MINER_A, "rig-01", HASH_A, 1_000_000, corr))
        .await;

    let count: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM block")
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(count, 1, "duplicate BlockFound must not insert twice");
}

#[tokio::test]
async fn block_accepted_without_prior_found_is_no_op() {
    let env = setup().await;
    let consumer = EventConsumer::new(env.db.clone(), cfg());

    let corr = CorrelationId::new_v4();
    // No prior BlockFound for HASH_B.
    consumer.handle_event(block_accepted(HASH_B, corr)).await;

    let count: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM block")
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "orphan BlockAccepted should not create a block row"
    );
}

#[tokio::test]
async fn share_rejected_persists_to_share_reject_not_share() {
    // M2: ShareRejected lands in `share_reject`, NOT `share`.
    // Wallet + worker rows ARE created so per-miner stats can
    // aggregate against them.
    let env = setup().await;
    let consumer = EventConsumer::new(env.db.clone(), cfg());

    let event = PoolEvent::ShareRejected {
        wallet: w(MINER_A),
        worker: katpool_domain::WorkerName::new("rig-01".to_owned()).unwrap(),
        reason: ShareRejectReason::Stale,
        ts: chrono::Utc::now(),
        correlation_id: CorrelationId::new_v4(),
    };
    consumer.handle_event(event).await;

    let share_count: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM share")
        .fetch_one(&env.db)
        .await
        .unwrap();
    let share_reject_count: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM share_reject")
        .fetch_one(&env.db)
        .await
        .unwrap();
    let wallet_count: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM wallet")
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(
        share_count, 0,
        "rejected shares must NOT land in the share table"
    );
    assert_eq!(
        share_reject_count, 1,
        "rejected shares must land in share_reject"
    );
    assert_eq!(
        wallet_count, 1,
        "M2 creates wallet rows for rejected shares so stats aggregate them"
    );
}

#[tokio::test]
async fn run_drains_broadcast_until_closed() {
    let env = setup().await;
    let (tx, rx) = broadcast::channel::<PoolEvent>(32);
    let consumer = EventConsumer::new(env.db.clone(), cfg());

    let handle = tokio::spawn(consumer.run(rx));

    tx.send(share_credited(MINER_A, "rig-01", 1024.0, 1_000_000))
        .unwrap();
    tx.send(share_credited(MINER_B, "rig-02", 2048.0, 1_000_001))
        .unwrap();
    // Drop the sender to close the channel.
    drop(tx);

    handle.await.unwrap().unwrap();

    // Both events landed.
    let count: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM share")
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn run_continues_after_broadcast_lag() {
    let env = setup().await;
    // Channel capacity = 2. We'll publish 5 events while the
    // consumer is blocked, so the receiver is guaranteed to see
    // RecvError::Lagged on its first recv.
    let (tx, rx) = broadcast::channel::<PoolEvent>(2);
    let consumer = EventConsumer::new(env.db.clone(), cfg());

    // Send 5 events BEFORE the consumer starts running, so the
    // first 3 are lost.
    for i in 0..5 {
        tx.send(share_credited(MINER_A, "rig-01", 1024.0, 1_000_000 + i))
            .unwrap();
    }

    let handle = tokio::spawn(consumer.run(rx));

    // Give the consumer a moment to drain.
    sleep(Duration::from_millis(200)).await;
    drop(tx);
    handle.await.unwrap().unwrap();

    // We expect exactly the trailing 2 events to have landed.
    // The first 3 were dropped; the consumer saw a `Lagged(3)`
    // and skipped to the buffer head.
    let count: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM share")
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(count, 2, "expected 2 shares after lag-skip; got {count}");
}
