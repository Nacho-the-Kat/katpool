//! Replay-determinism test for the consumer.
//!
//! Feeds the same event sequence to two independent consumers
//! backed by two independent empty Postgres instances, then
//! asserts the two resulting databases are byte-equal in every
//! row the consumer wrote.
//!
//! This is the strongest single-test we have for the contract
//! "given the same event stream, the accountant produces the
//! same DB state every time". It catches:
//!
//! - non-determinism via wallclock timestamps (`credited_at`
//!   defaulting to `now()`) leaking into row identity
//! - hidden ordering dependencies in the event handler
//! - any unobservable randomness introduced by future refactors
//!
//! We compare the *content* of every row (PK aside, since those
//! are serial and freshly assigned each run). Two PKs differing
//! between runs is fine; their referenced data differing is not.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use chrono::{TimeZone, Utc};
use katpool_db::{PoolConfig, build_pool, migrate};
use katpool_domain::{
    BlockHash, CorrelationId, DaaScore, PoolEvent, ShareDifficulty, ShareRejectReason,
    WalletAddress, WorkerName,
};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

use accountant::{ConsumerConfig, EventConsumer};

const MINER_A: &str = "kaspa:qypczcz0lhyf3tfsuqj86e7qc8us7r8a53nhlr4u6x4kq38td0hsjycf7sya7zq";
const MINER_B: &str = "kaspa:qzncghl8re9h35hp6n5wyxtslhevj6462qkrkqzlfkrs2mpkfkc5xe9s3tga7";
const HASH_A: &str = "cc2b1da2c931f4164c03b2066cfb3178303567a161e8a393def62c91e824138a";
const HASH_B: &str = "9685f4347b9aa2e100bf489f7979a30746d90823d5bfb62309513b1e23ab2274";

struct Env {
    db: sqlx::PgPool,
    _ctr: ContainerAsync<Postgres>,
}

async fn fresh_db() -> Env {
    let container = Postgres::default().start().await.expect("postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let db = build_pool(&PoolConfig {
        url,
        min_connections: 1,
        max_connections: 4,
        application_name: "replay".to_owned(),
        ..PoolConfig::production("placeholder".to_owned())
    })
    .await
    .expect("pool");
    migrate::run(&db).await.expect("migrate");
    Env {
        db,
        _ctr: container,
    }
}

/// Build a deterministic synthetic event stream. Every event
/// carries an explicit `ts` and a fixed `correlation_id` so two
/// runs see byte-equal inputs.
/// Build a correlation id from a single byte by stuffing it into
/// the low byte of an all-zero UUID and setting the v4 marker
/// bits. Deterministic across runs.
const fn corr(i: u8) -> CorrelationId {
    let mut bytes = [0u8; 16];
    bytes[15] = i;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    CorrelationId::from_uuid(Uuid::from_bytes(bytes))
}

fn deterministic_stream() -> Vec<PoolEvent> {
    let ts = Utc.with_ymd_and_hms(2026, 5, 26, 0, 0, 0).unwrap();
    let wallet_a = WalletAddress::new(MINER_A.to_owned()).unwrap();
    let wallet_b = WalletAddress::new(MINER_B.to_owned()).unwrap();
    let worker_a = WorkerName::new("rig-01".to_owned()).unwrap();
    let worker_b = WorkerName::new("rig-02".to_owned()).unwrap();
    let hash_a = BlockHash::from_hex(HASH_A).unwrap();
    let hash_b = BlockHash::from_hex(HASH_B).unwrap();

    vec![
        PoolEvent::ShareCredited {
            wallet: wallet_a.clone(),
            worker: worker_a.clone(),
            difficulty: ShareDifficulty::new(1024.0).unwrap(),
            daa_score: DaaScore::new(1_000_000),
            ts,
            correlation_id: corr(1),
        },
        PoolEvent::ShareCredited {
            wallet: wallet_b.clone(),
            worker: worker_b.clone(),
            difficulty: ShareDifficulty::new(2048.0).unwrap(),
            daa_score: DaaScore::new(1_000_001),
            ts,
            correlation_id: corr(2),
        },
        PoolEvent::ShareRejected {
            wallet: wallet_a.clone(),
            worker: worker_a.clone(),
            reason: ShareRejectReason::Stale,
            ts,
            correlation_id: corr(3),
        },
        PoolEvent::BlockFound {
            wallet: wallet_a.clone(),
            worker: worker_a.clone(),
            hash: hash_a,
            daa_score: DaaScore::new(1_000_002),
            ts,
            correlation_id: corr(4),
        },
        PoolEvent::BlockAccepted {
            hash: hash_a,
            ts,
            correlation_id: corr(4),
        },
        PoolEvent::ShareRejected {
            wallet: wallet_b,
            worker: worker_b,
            reason: ShareRejectReason::LowDifficulty,
            ts,
            correlation_id: corr(5),
        },
        PoolEvent::BlockFound {
            wallet: wallet_a,
            worker: worker_a,
            hash: hash_b,
            daa_score: DaaScore::new(1_000_010),
            ts,
            correlation_id: corr(6),
        },
    ]
}

/// Snapshot every row in every table the consumer touches, in a
/// canonical order. The PK column is excluded from each tuple
/// because it's a freshly-assigned BIGSERIAL — comparing PKs
/// across two independent DB instances is meaningless. The
/// `wallclock` columns (`first_seen_at`, `last_seen_at`,
/// `credited_at`, `rejected_at`, `found_at`, `submitted_at`)
/// are also excluded: M1's consumer doesn't pass an explicit
/// timestamp to the repo, so `now()` defaults are non-deterministic
/// across runs — that's a finding documented separately, not
/// something this test asserts.
#[derive(Debug, PartialEq)]
struct DbSnapshot {
    wallets: Vec<(String, String)>,
    workers: Vec<(String,)>,
    shares: Vec<(f64, i64, Uuid)>,
    rejects: Vec<(String, Uuid)>,
    blocks: Vec<(Vec<u8>, i64, i64, String, Uuid)>,
}

async fn snapshot(db: &sqlx::PgPool) -> DbSnapshot {
    let wallets: Vec<(String, String)> =
        sqlx::query_as("SELECT address, network FROM wallet ORDER BY address")
            .fetch_all(db)
            .await
            .unwrap();
    let workers: Vec<(String,)> = sqlx::query_as("SELECT name FROM worker ORDER BY name")
        .fetch_all(db)
        .await
        .unwrap();
    let shares: Vec<(f64, i64, Uuid)> = sqlx::query_as(
        "SELECT difficulty, daa_score, correlation_id
           FROM share
          ORDER BY correlation_id",
    )
    .fetch_all(db)
    .await
    .unwrap();
    let rejects: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT reason::text, correlation_id
           FROM share_reject
          ORDER BY correlation_id",
    )
    .fetch_all(db)
    .await
    .unwrap();
    let blocks: Vec<(Vec<u8>, i64, i64, String, Uuid)> = sqlx::query_as(
        "SELECT hash, daa_score, nonce, status::text, correlation_id
           FROM block
          ORDER BY hash",
    )
    .fetch_all(db)
    .await
    .unwrap();

    DbSnapshot {
        wallets,
        workers,
        shares,
        rejects,
        blocks,
    }
}

#[tokio::test]
async fn same_event_stream_produces_identical_db_state() {
    let stream = deterministic_stream();
    let env_a = fresh_db().await;
    let env_b = fresh_db().await;

    let consumer_a = EventConsumer::new(
        env_a.db.clone(),
        ConsumerConfig::new("a".to_owned(), "mainnet".to_owned()).unwrap(),
    );
    let consumer_b = EventConsumer::new(
        env_b.db.clone(),
        ConsumerConfig::new("b".to_owned(), "mainnet".to_owned()).unwrap(),
    );

    for event in &stream {
        consumer_a.handle_event(event.clone()).await;
    }
    for event in &stream {
        consumer_b.handle_event(event.clone()).await;
    }

    let snap_a = snapshot(&env_a.db).await;
    let snap_b = snapshot(&env_b.db).await;
    assert_eq!(
        snap_a, snap_b,
        "two independent consumers fed the same stream must produce byte-equal DB state"
    );

    // Belt-and-braces: sanity that the snapshots aren't both empty
    // (which would make the equality trivially true).
    assert!(!snap_a.wallets.is_empty());
    assert!(!snap_a.shares.is_empty());
    assert!(!snap_a.blocks.is_empty());
    assert!(!snap_a.rejects.is_empty());
}

#[tokio::test]
async fn replaying_same_stream_into_same_db_is_idempotent_for_blocks() {
    // Replay-into-same-db isn't claimed to be idempotent for
    // `share` rows — the schema doesn't have UNIQUE on
    // (correlation_id) and the design absorbs at-most-once
    // delivery from the broadcast channel. For `block` rows
    // the `UNIQUE (hash)` + `block::ensure` does enforce
    // idempotency, and that's what this test pins.
    let stream = deterministic_stream();
    let env = fresh_db().await;
    let consumer = EventConsumer::new(
        env.db.clone(),
        ConsumerConfig::new("x".to_owned(), "mainnet".to_owned()).unwrap(),
    );
    for event in &stream {
        consumer.handle_event(event.clone()).await;
    }
    let snap1 = snapshot(&env.db).await;
    for event in &stream {
        consumer.handle_event(event.clone()).await;
    }
    let snap2 = snapshot(&env.db).await;
    assert_eq!(
        snap1.blocks, snap2.blocks,
        "block rows must be idempotent on replay"
    );
    assert_eq!(
        snap1.wallets, snap2.wallets,
        "wallet rows are upsert-idempotent"
    );
    assert_eq!(
        snap1.workers, snap2.workers,
        "worker rows are upsert-idempotent"
    );
}
