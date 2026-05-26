//! Shared testcontainer + connection-pool harness for every
//! `katpool-import-legacy` integration test.
//!
//! Each test gets a fresh container (and therefore fresh legacy +
//! target databases). The container is dropped when [`Env`] is.

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use katpool_db::{PoolConfig, build_pool, migrate};
use testcontainers::ContainerAsync;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

/// Stable production-shaped wallet addresses used across tests.
pub const POOL_ADDR: &str = "kaspa:qz4j8mu269z8llgcczmfukm9fan2fq822kzxu4cfukd5fqrhxpsv2zhs9jxnp";
pub const MINER_A: &str = "kaspa:qypczcz0lhyf3tfsuqj86e7qc8us7r8a53nhlr4u6x4kq38td0hsjycf7sya7zq";
pub const MINER_B: &str = "kaspa:qzncghl8re9h35hp6n5wyxtslhevj6462qkrkqzlfkrs2mpkfkc5xe9s3tga7";

/// Block-hash hex strings reused by tests that touch `tx_hash` /
/// `mined_block_hash`. Valid 64-char lowercase hex strings.
pub const VALID_HASH_A: &str = "cc2b1da2c931f4164c03b2066cfb3178303567a161e8a393def62c91e824138a";
pub const VALID_HASH_B: &str = "9685f4347b9aa2e100bf489f7979a30746d90823d5bfb62309513b1e23ab2274";
pub const VALID_HASH_C: &str = "c123f9a7e37b7404062aa84239013bc733286b23319d4296bbc20b764ee8782a";

/// A complete fresh environment: one container, two databases, both
/// pools ready. The legacy schema is loaded; the target schema is
/// migrated. Keep `_ctr` alive for the duration of the test.
pub struct Env {
    pub legacy: sqlx::PgPool,
    pub target: sqlx::PgPool,
    _ctr: ContainerAsync<Postgres>,
}

/// Stand up the testcontainer and both pools.
pub async fn setup() -> Env {
    let init_sql = format!(
        "CREATE DATABASE legacy_test;\n\
         CREATE DATABASE target_test;\n\
         \\c legacy_test\n\
         {legacy_schema}",
        legacy_schema = include_str!("../fixtures/legacy_schema.sql"),
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
