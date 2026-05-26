//! End-to-end importer test for the `block_details` transform.
//!
//! Uses a single Postgres testcontainer with two databases on it
//! (`legacy_test` + `target_test`). The legacy DB is seeded with a
//! handful of `block_details` rows in known shapes; the target DB
//! is fresh. The test invokes the transform directly (not via the
//! binary's `main`) so we can assert on `TransformStats` without
//! parsing the binary's stdout JSON.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use katpool_db::repo::block::BlockStatus;
use katpool_db::repo::{block, wallet, worker};
use katpool_db::{PoolConfig, build_pool, migrate};
use katpool_domain::{BlockHash, WalletAddress, WorkerName};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use katpool_import_legacy::transform::blocks;

/// A complete fresh environment: one container, two databases, both
/// pools ready. The legacy schema is loaded; the target schema is
/// migrated. Keep `_ctr` alive for the duration of the test.
struct Env {
    legacy: sqlx::PgPool,
    target: sqlx::PgPool,
    _ctr: ContainerAsync<Postgres>,
}

async fn setup() -> Env {
    // Run the postgres container with init SQL that creates both
    // databases up front. The testcontainers-modules Postgres image
    // executes anything in /docker-entrypoint-initdb.d/ on first
    // start, so a single `with_init_sql` call wires both schemas
    // into the right places before we even connect.
    let init_sql = format!(
        "CREATE DATABASE legacy_test;\n\
         CREATE DATABASE target_test;\n\
         \\c legacy_test\n\
         {legacy_schema}",
        legacy_schema = include_str!("fixtures/legacy_schema.sql"),
    );

    let container = Postgres::default()
        .with_init_sql(init_sql.into_bytes())
        .start()
        .await
        .expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");

    let legacy_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/legacy_test");
    let target_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/target_test");

    let legacy = build_pool(&PoolConfig {
        url: legacy_url,
        min_connections: 1,
        max_connections: 4,
        application_name: "katpool-import-legacy[test-legacy]".to_owned(),
        ..PoolConfig::production("placeholder".to_owned())
    })
    .await
    .expect("legacy pool");

    let target = build_pool(&PoolConfig {
        url: target_url,
        min_connections: 1,
        max_connections: 4,
        application_name: "katpool-import-legacy[test-target]".to_owned(),
        ..PoolConfig::production("placeholder".to_owned())
    })
    .await
    .expect("target pool");

    migrate::run(&target)
        .await
        .expect("apply target migrations");

    Env {
        legacy,
        target,
        _ctr: container,
    }
}

/// Seed a single `block_details` row.
#[allow(clippy::too_many_arguments)]
async fn seed_block(
    legacy: &sqlx::PgPool,
    mined_block_hash: &str,
    miner_id: Option<&str>,
    pool_address: Option<&str>,
    wallet: Option<&str>,
    daa_score: Option<&str>,
    miner_reward: i64,
) {
    sqlx::query(
        "INSERT INTO block_details
            (mined_block_hash, miner_id, pool_address, wallet, daa_score, miner_reward)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(mined_block_hash)
    .bind(miner_id)
    .bind(pool_address)
    .bind(wallet)
    .bind(daa_score)
    .bind(miner_reward)
    .execute(legacy)
    .await
    .expect("seed block_details row");
}

/// Sample data shaped like the production legacy DB on this VPS.
const POOL_ADDR: &str = "kaspa:qz4j8mu269z8llgcczmfukm9fan2fq822kzxu4cfukd5fqrhxpsv2zhs9jxnp";
const MINER_A: &str = "kaspa:qypczcz0lhyf3tfsuqj86e7qc8us7r8a53nhlr4u6x4kq38td0hsjycf7sya7zq";
const MINER_B: &str = "kaspa:qzncghl8re9h35hp6n5wyxtslhevj6462qkrkqzlfkrs2mpkfkc5xe9s3tga7";

const VALID_HASH_A: &str = "cc2b1da2c931f4164c03b2066cfb3178303567a161e8a393def62c91e824138a";
const VALID_HASH_B: &str = "9685f4347b9aa2e100bf489f7979a30746d90823d5bfb62309513b1e23ab2274";
const VALID_HASH_C: &str = "c123f9a7e37b7404062aa84239013bc733286b23319d4296bbc20b764ee8782a";

#[tokio::test]
async fn blocks_importer_inserts_then_idempotent_skips() {
    let env = setup().await;

    seed_block(
        &env.legacy,
        VALID_HASH_A,
        Some("JANKS5Pro"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("427337124"),
        275_000_000,
    )
    .await;
    seed_block(
        &env.legacy,
        VALID_HASH_B,
        Some("KS5P02"),
        Some(POOL_ADDR),
        Some(MINER_B),
        Some("427337293"),
        275_079_404,
    )
    .await;

    let stats = blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("first run");
    assert_eq!(stats.read, 2);
    assert_eq!(stats.inserted, 2);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.rejected, 0);

    // Idempotent re-run.
    let stats2 = blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("second run");
    assert_eq!(stats2.read, 2);
    assert_eq!(stats2.inserted, 0);
    assert_eq!(stats2.skipped, 2);
    assert_eq!(stats2.rejected, 0);
}

#[tokio::test]
async fn blocks_importer_creates_wallet_and_worker_rows() {
    let env = setup().await;
    seed_block(
        &env.legacy,
        VALID_HASH_A,
        Some("JANKS5Pro"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("427337124"),
        275_000_000,
    )
    .await;
    seed_block(
        &env.legacy,
        VALID_HASH_B,
        Some("JANKS5Pro"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("427337125"),
        275_000_001,
    )
    .await;
    seed_block(
        &env.legacy,
        VALID_HASH_C,
        Some("KS5P02"),
        Some(POOL_ADDR),
        Some(MINER_B),
        Some("427337293"),
        275_079_404,
    )
    .await;

    blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("run");

    let miner_a = WalletAddress::new(MINER_A).expect("valid");
    let miner_b = WalletAddress::new(MINER_B).expect("valid");

    let wa = wallet::find_by_address(&env.target, &miner_a)
        .await
        .expect("find a")
        .expect("present");
    let wb = wallet::find_by_address(&env.target, &miner_b)
        .await
        .expect("find b")
        .expect("present");
    assert_ne!(wa.id, wb.id);

    let workers_a = worker::list_for_wallet(&env.target, wa.id)
        .await
        .expect("workers a");
    assert_eq!(workers_a.len(), 1);
    assert_eq!(workers_a[0].name, "JANKS5Pro");

    let workers_b = worker::list_for_wallet(&env.target, wb.id)
        .await
        .expect("workers b");
    assert_eq!(workers_b.len(), 1);
    assert_eq!(workers_b[0].name, "KS5P02");
}

#[tokio::test]
async fn imported_blocks_land_in_matured_state_with_reward() {
    let env = setup().await;
    seed_block(
        &env.legacy,
        VALID_HASH_A,
        Some("JANKS5Pro"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("427337124"),
        275_000_000,
    )
    .await;

    blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("run");

    let hash = BlockHash::from_hex(VALID_HASH_A).expect("hex");
    let b = block::find_by_hash(&env.target, hash)
        .await
        .expect("find")
        .expect("present");

    assert_eq!(b.status, BlockStatus::Matured);
    assert_eq!(b.daa_score, 427_337_124);
    assert_eq!(b.miner_reward_sompi, Some(275_000_000));
    assert!(b.submitted_at.is_some());
    assert!(b.confirmed_at.is_some());
    assert!(b.matured_at.is_some());
}

#[tokio::test]
async fn correlation_id_is_deterministic_across_runs() {
    let env = setup().await;
    seed_block(
        &env.legacy,
        VALID_HASH_A,
        Some("JANKS5Pro"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("1"),
        1,
    )
    .await;
    blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("first run");

    let hash = BlockHash::from_hex(VALID_HASH_A).expect("hex");
    let first = block::find_by_hash(&env.target, hash)
        .await
        .expect("find")
        .expect("present");
    let first_cid = first.correlation_id;

    // Wipe target and re-run; correlation id should be identical.
    sqlx::query("TRUNCATE block, worker, wallet RESTART IDENTITY CASCADE")
        .execute(&env.target)
        .await
        .expect("truncate");
    blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("second run");
    let second = block::find_by_hash(&env.target, hash)
        .await
        .expect("find")
        .expect("present");
    assert_eq!(first_cid, second.correlation_id);
}

#[tokio::test]
async fn rejects_rows_with_unparseable_data() {
    let env = setup().await;

    // 1. Missing wallet.
    seed_block(
        &env.legacy,
        VALID_HASH_A,
        Some("rig-1"),
        Some(POOL_ADDR),
        None,
        Some("1"),
        0,
    )
    .await;
    // 2. Hash not 64-char hex (32-char).
    seed_block(
        &env.legacy,
        "aabbccddaabbccddaabbccddaabbccdd",
        Some("rig-2"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("1"),
        0,
    )
    .await;
    // 3. daa_score not parseable as u64.
    seed_block(
        &env.legacy,
        VALID_HASH_B,
        Some("rig-3"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("not-a-number"),
        0,
    )
    .await;
    // 4. Worker name with forbidden chars.
    seed_block(
        &env.legacy,
        VALID_HASH_C,
        Some("bad worker name"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("1"),
        0,
    )
    .await;
    // 5. Successful row.
    seed_block(
        &env.legacy,
        "0".repeat(64).as_str(),
        Some("rig-good"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("1"),
        100,
    )
    .await;

    let stats = blocks::run(&env.legacy, &env.target, false)
        .await
        .expect("run");
    assert_eq!(stats.read, 5);
    assert_eq!(stats.inserted, 1);
    assert_eq!(stats.rejected, 4);
}

#[tokio::test]
async fn dry_run_writes_nothing_but_counts_correctly() {
    let env = setup().await;
    seed_block(
        &env.legacy,
        VALID_HASH_A,
        Some("JANKS5Pro"),
        Some(POOL_ADDR),
        Some(MINER_A),
        Some("1"),
        100,
    )
    .await;

    let stats = blocks::run(&env.legacy, &env.target, true)
        .await
        .expect("dry run");
    assert_eq!(stats.read, 1);
    assert_eq!(stats.inserted, 1); // counted as if-it-had-inserted

    let hash = BlockHash::from_hex(VALID_HASH_A).expect("hex");
    let b = block::find_by_hash(&env.target, hash).await.expect("query");
    assert!(b.is_none(), "dry run must not have written anything");

    let _ = WorkerName::new("rig")
        .expect("just to silence the unused import — kept for parity with other tests");
}
