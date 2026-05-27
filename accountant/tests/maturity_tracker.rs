//! Tests for `accountant::MaturityTracker` against an in-memory
//! `KaspadClient` fake.
//!
//! The tracker's state machine is the bulk of the code under
//! test. The kaspad client is a trait, so we drive the tracker
//! by manipulating a `FakeKaspad` from the outside — entirely
//! deterministic, no network.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic,
    clippy::similar_names,
    clippy::cast_possible_wrap,
    // Test-only helper constants and synthetic byte casts; the
    // values are bounded to 0..5 so the u8 cast is exact.
    clippy::missing_const_for_fn,
    clippy::cast_possible_truncation
)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use katpool_db::repo::block::{self, BlockStatus};
use katpool_db::repo::{share, wallet, worker};
use katpool_domain::{
    BlockHash, CorrelationId, DaaScore, ShareDifficulty, WalletAddress, WorkerName,
};
use tokio::sync::{Mutex, watch};

use accountant::{
    AllocationEngine, BlockInfo, FeeConfig, KaspadClient, KaspadError, MaturityConfig,
    MaturityTracker, StaticTierClassifier,
};

mod common;
use common::{HASH_A, HASH_B, MINER_A, setup};

const NETWORK: &str = "mainnet";

/// In-memory `KaspadClient` whose state is driven from tests via
/// the `set_*` mutators.
#[derive(Debug, Default)]
struct FakeKaspad {
    state: Mutex<FakeState>,
}

#[derive(Debug, Default)]
struct FakeState {
    virtual_blue_score: u64,
    blocks: HashMap<BlockHash, BlockInfo>,
    fail_next_get_block: bool,
    fail_next_virtual_blue: bool,
}

impl FakeKaspad {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    async fn set_virtual_blue_score(&self, v: u64) {
        self.state.lock().await.virtual_blue_score = v;
    }

    async fn add_block(&self, info: BlockInfo) {
        self.state.lock().await.blocks.insert(info.hash, info);
    }
}

#[async_trait]
impl KaspadClient for FakeKaspad {
    async fn get_virtual_blue_score(&self) -> Result<u64, KaspadError> {
        let mut s = self.state.lock().await;
        if s.fail_next_virtual_blue {
            s.fail_next_virtual_blue = false;
            return Err(KaspadError::Transport("test-injected".to_owned()));
        }
        Ok(s.virtual_blue_score)
    }

    async fn get_block(&self, hash: BlockHash) -> Result<Option<BlockInfo>, KaspadError> {
        let mut s = self.state.lock().await;
        if s.fail_next_get_block {
            s.fail_next_get_block = false;
            return Err(KaspadError::Transport("test-injected".to_owned()));
        }
        Ok(s.blocks.get(&hash).copied())
    }
}

async fn ensure_wallet_worker(
    db: &sqlx::PgPool,
    wallet_str: &str,
    worker_str: &str,
) -> (katpool_db::repo::WalletId, katpool_db::repo::WorkerId) {
    let mut tx = db.begin().await.unwrap();
    let w = wallet::ensure(
        &mut *tx,
        &WalletAddress::new(wallet_str.to_owned()).unwrap(),
        NETWORK,
    )
    .await
    .unwrap();
    let wk = worker::ensure(
        &mut *tx,
        w.id,
        &WorkerName::new(worker_str.to_owned()).unwrap(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (w.id, wk.id)
}

async fn seed_share(
    db: &sqlx::PgPool,
    w_id: katpool_db::repo::WalletId,
    wk_id: katpool_db::repo::WorkerId,
    difficulty: f64,
    daa: u64,
) {
    share::insert_credited(
        db,
        w_id,
        wk_id,
        None,
        ShareDifficulty::new(difficulty).unwrap(),
        DaaScore::new(daa),
        CorrelationId::new_v4(),
    )
    .await
    .unwrap();
}

/// Insert a block in `submitted_to_node` state (mirrors the
/// consumer's post-BlockAccepted state).
async fn insert_submitted(
    db: &sqlx::PgPool,
    hash: BlockHash,
    finder_w: katpool_db::repo::WalletId,
    finder_wk: katpool_db::repo::WorkerId,
    daa: u64,
) {
    let _ = block::ensure(
        db,
        hash,
        finder_w,
        finder_wk,
        DaaScore::new(daa),
        0,
        CorrelationId::new_v4(),
    )
    .await
    .unwrap();
    block::mark_submitted(db, hash).await.unwrap();
}

fn build_tracker(
    db: sqlx::PgPool,
    kaspad: Arc<FakeKaspad>,
    cfg: MaturityConfig,
) -> MaturityTracker {
    let fee = FeeConfig::new(75).unwrap();
    let engine = Arc::new(AllocationEngine::new(
        db.clone(),
        fee,
        Arc::new(StaticTierClassifier::standard()),
        "test".to_owned(),
    ));
    MaturityTracker::new(db, kaspad as _, engine, cfg, "test".to_owned())
}

fn default_cfg() -> MaturityConfig {
    MaturityConfig {
        poll_interval: Duration::from_millis(50),
        maturity_depth: 100,
        window_daa_span: 600,
        batch_size: 200,
    }
}

// ---------- transitions --------------------------------------------

#[tokio::test]
async fn submitted_to_node_transitions_to_confirmed_blue_when_blue() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let h = BlockHash::from_hex(HASH_A).unwrap();
    insert_submitted(&env.db, h, w, wk, 1_000_000).await;

    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_500).await;
    kaspad
        .add_block(BlockInfo {
            hash: h,
            blue_score: 100_400, // not yet deep enough
            is_blue: true,
            coinbase_reward_sompi: 0,
            daa_score: 1_000_010,
        })
        .await;
    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(stats.confirmed_blue, 1);
    assert_eq!(stats.matured, 0);
    assert_eq!(stats.orphaned, 0);

    let blk = block::find_by_hash(&env.db, h).await.unwrap().unwrap();
    assert_eq!(blk.status, BlockStatus::ConfirmedBlue);
    assert_eq!(blk.blue_score, Some(100_400));
}

#[tokio::test]
async fn submitted_to_node_stays_when_kaspad_doesnt_know_block_yet() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let h = BlockHash::from_hex(HASH_A).unwrap();
    insert_submitted(&env.db, h, w, wk, 1_000_000).await;

    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_500).await;
    // No block added.

    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(stats.still_waiting, 1);
    let blk = block::find_by_hash(&env.db, h).await.unwrap().unwrap();
    assert_eq!(blk.status, BlockStatus::SubmittedToNode);
}

#[tokio::test]
async fn submitted_to_node_stays_when_block_seen_but_red() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let h = BlockHash::from_hex(HASH_A).unwrap();
    insert_submitted(&env.db, h, w, wk, 1_000_000).await;

    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_500).await;
    kaspad
        .add_block(BlockInfo {
            hash: h,
            blue_score: 0,
            is_blue: false,
            coinbase_reward_sompi: 0,
            daa_score: 1_000_010,
        })
        .await;
    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(stats.still_waiting, 1);
    let blk = block::find_by_hash(&env.db, h).await.unwrap().unwrap();
    assert_eq!(blk.status, BlockStatus::SubmittedToNode);
}

#[tokio::test]
async fn confirmed_blue_matures_when_depth_reached_and_triggers_engine() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let h = BlockHash::from_hex(HASH_A).unwrap();
    // Seed enough shares for the engine to do real work.
    for i in 0..5 {
        seed_share(&env.db, w, wk, 1024.0, 1_000_000 + i).await;
    }
    insert_submitted(&env.db, h, w, wk, 1_000_010).await;
    block::mark_confirmed_blue(&env.db, h, 100_400)
        .await
        .unwrap();

    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_500).await; // depth 100
    kaspad
        .add_block(BlockInfo {
            hash: h,
            blue_score: 100_400,
            is_blue: true,
            coinbase_reward_sompi: 500_000_000,
            daa_score: 1_000_010,
        })
        .await;
    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(stats.matured, 1);
    let blk = block::find_by_hash(&env.db, h).await.unwrap().unwrap();
    assert_eq!(blk.status, BlockStatus::Matured);
    assert_eq!(blk.miner_reward_sompi, Some(500_000_000));

    // Engine ran: allocations exist for this block.
    let n: i64 =
        sqlx::query_scalar("SELECT count(*)::bigint FROM share_allocation WHERE block_id = $1")
            .bind(blk.id.0)
            .fetch_one(&env.db)
            .await
            .unwrap();
    assert_eq!(n, 1, "exactly one wallet contributed → one allocation");
}

#[tokio::test]
async fn confirmed_blue_waits_when_depth_insufficient() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let h = BlockHash::from_hex(HASH_A).unwrap();
    insert_submitted(&env.db, h, w, wk, 1_000_010).await;
    block::mark_confirmed_blue(&env.db, h, 100_400)
        .await
        .unwrap();

    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_499).await; // depth 99, < 100
    kaspad
        .add_block(BlockInfo {
            hash: h,
            blue_score: 100_400,
            is_blue: true,
            coinbase_reward_sompi: 500_000_000,
            daa_score: 1_000_010,
        })
        .await;
    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(stats.still_waiting, 1);
    assert_eq!(stats.matured, 0);
    let blk = block::find_by_hash(&env.db, h).await.unwrap().unwrap();
    assert_eq!(blk.status, BlockStatus::ConfirmedBlue);
}

#[tokio::test]
async fn confirmed_blue_orphans_when_block_disappears_from_dag() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let h = BlockHash::from_hex(HASH_A).unwrap();
    insert_submitted(&env.db, h, w, wk, 1_000_010).await;
    block::mark_confirmed_blue(&env.db, h, 100_400)
        .await
        .unwrap();

    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_500).await;
    // Don't add the block — kaspad has lost it.
    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(stats.orphaned, 1);
    let blk = block::find_by_hash(&env.db, h).await.unwrap().unwrap();
    assert_eq!(blk.status, BlockStatus::Orphaned);
}

#[tokio::test]
async fn confirmed_blue_orphans_on_reorg_to_red() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let h = BlockHash::from_hex(HASH_A).unwrap();
    insert_submitted(&env.db, h, w, wk, 1_000_010).await;
    block::mark_confirmed_blue(&env.db, h, 100_400)
        .await
        .unwrap();

    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_500).await;
    kaspad
        .add_block(BlockInfo {
            hash: h,
            blue_score: 100_400,
            is_blue: false, // re-orged out
            coinbase_reward_sompi: 0,
            daa_score: 1_000_010,
        })
        .await;
    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(stats.orphaned, 1);
    let blk = block::find_by_hash(&env.db, h).await.unwrap().unwrap();
    assert_eq!(blk.status, BlockStatus::Orphaned);
}

// ---------- error isolation ----------------------------------------

#[tokio::test]
async fn whole_sweep_fails_when_virtual_blue_score_query_errors() {
    let env = setup().await;
    let kaspad = FakeKaspad::new();
    {
        let mut s = kaspad.state.lock().await;
        s.fail_next_virtual_blue = true;
    }
    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let err = tracker
        .run_once()
        .await
        .expect_err("transport error → sweep failure");
    let msg = format!("{err}");
    assert!(msg.contains("kaspad") || msg.contains("transport"), "{msg}");
}

#[tokio::test]
async fn per_block_get_block_error_is_isolated() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let h_a = BlockHash::from_hex(HASH_A).unwrap();
    let h_b = BlockHash::from_hex(HASH_B).unwrap();
    insert_submitted(&env.db, h_a, w, wk, 1_000_000).await;
    insert_submitted(&env.db, h_b, w, wk, 1_000_010).await;

    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_500).await;
    kaspad
        .add_block(BlockInfo {
            hash: h_b,
            blue_score: 100_400,
            is_blue: true,
            coinbase_reward_sompi: 0,
            daa_score: 1_000_010,
        })
        .await;
    // Inject a one-shot error.
    {
        let mut s = kaspad.state.lock().await;
        s.fail_next_get_block = true;
    }
    let tracker = build_tracker(env.db.clone(), kaspad, default_cfg());
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(stats.errors, 1, "first block sees the injected error");
    // The other block proceeded normally.
    assert!(stats.confirmed_blue + stats.still_waiting >= 1);
}

// ---------- batch limit --------------------------------------------

#[tokio::test]
async fn batch_size_limits_blocks_processed_per_sweep() {
    let env = setup().await;
    let (w, wk) = ensure_wallet_worker(&env.db, MINER_A, "rig-01").await;
    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(100_500).await;

    // Seed 5 submitted_to_node blocks.
    for i in 0..5u64 {
        // Manufacture distinct hashes via a deterministic prefix
        // + index byte (collision-free for 0..255).
        let mut bytes = [0u8; 32];
        bytes[31] = i as u8 + 1;
        let h = BlockHash::from_bytes(bytes);
        insert_submitted(&env.db, h, w, wk, 1_000_000 + i).await;
        // None of them are in kaspad → still_waiting.
    }

    let cfg = MaturityConfig {
        batch_size: 2,
        ..default_cfg()
    };
    let tracker = build_tracker(env.db.clone(), kaspad, cfg);
    let stats = tracker.run_once().await.unwrap();
    assert_eq!(
        stats.still_waiting + stats.confirmed_blue + stats.matured + stats.orphaned + stats.errors,
        2,
        "exactly batch_size blocks processed per sweep"
    );
}

// ---------- run_loop shutdown --------------------------------------

#[tokio::test]
async fn run_loop_exits_cleanly_on_shutdown_signal() {
    let env = setup().await;
    let kaspad = FakeKaspad::new();
    kaspad.set_virtual_blue_score(0).await;

    let cfg = MaturityConfig {
        poll_interval: Duration::from_millis(50),
        ..default_cfg()
    };
    let tracker = build_tracker(env.db.clone(), kaspad, cfg);

    let (tx, rx) = watch::channel(false);
    let handle = tokio::spawn(tracker.run_loop(rx));

    // Let at least one tick elapse.
    tokio::time::sleep(Duration::from_millis(150)).await;
    tx.send(true).unwrap();

    let result = tokio::time::timeout(Duration::from_secs(1), handle).await;
    assert!(result.is_ok(), "loop didn't shut down within 1s");
    assert!(result.unwrap().unwrap().is_ok(), "loop returned an error");
}
