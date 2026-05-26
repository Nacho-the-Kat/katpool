//! Read-side: row types and queries against the legacy
//! `katpool_mainnet` database.
//!
//! Every type here is `sqlx::FromRow` mapped 1:1 against a row of one
//! legacy table. Nothing in this module mutates the source; the
//! importer connects with a read-only role in production.
//!
//! Individual columns are intentionally undocumented at the field
//! level — the [legacy schema reference](../../../docs/db-schema.md)
//! and the original `katpool-app` repository are the canonical
//! sources for the legacy column semantics.

#![allow(missing_docs)]

use chrono::NaiveDateTime;

/// One row of legacy `block_details`.
///
/// Schema mismatch notes:
/// - `pool_address` is the **pool wallet** (constant across all rows
///   from the same pool deployment), confusingly named in the legacy
///   schema. We ignore it; the new schema's notion of "pool address"
///   lives in environment config, not in per-block rows.
/// - `wallet` is the **miner's wallet** — the actual recipient
///   identity. Maps to `wallet.address` in the new schema.
/// - `miner_id` is the worker name (e.g. `KS5P02`). Maps to
///   `worker.name`.
/// - `daa_score` is `varchar(255)` in the legacy schema (!). The
///   new schema's `block.daa_score` is `BIGINT`. We parse on the
///   way through.
/// - `reward_block_hash` is the on-chain coinbase tx hash of the
///   reward output for this block; not currently used by the new
///   schema (we capture reward at maturity directly).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LegacyBlockDetail {
    /// 32-byte block hash, hex-encoded `varchar(255)` in the legacy schema.
    pub mined_block_hash: String,
    /// Worker name; nullable in the legacy schema (rare in practice).
    pub miner_id: Option<String>,
    /// Pool's own wallet — ignored by the importer.
    pub pool_address: Option<String>,
    /// Miner's wallet bech32.
    pub wallet: Option<String>,
    /// DAA score, stored as varchar.
    pub daa_score: Option<String>,
    /// Naive timestamp (no TZ; presumed UTC).
    pub timestamp: Option<NaiveDateTime>,
    /// Coinbase-reward block-hash linkage.
    #[allow(dead_code)] // we capture reward at maturity in the new schema
    pub reward_block_hash: Option<String>,
    /// Coinbase reward in sompi.
    pub miner_reward: i64,
}

/// One row of legacy `miners_balance`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LegacyMinersBalance {
    pub id: String,
    pub miner_id: Option<String>,
    pub wallet: Option<String>,
    pub balance: Option<sqlx::types::BigDecimal>,
    pub nacho_rebate_kas: Option<sqlx::types::BigDecimal>,
}

/// One row of legacy `payments`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LegacyPayment {
    pub id: i32,
    pub wallet_address: Vec<String>,
    pub amount: i64,
    pub timestamp: Option<NaiveDateTime>,
    pub transaction_hash: String,
}

/// One row of legacy `nacho_payments`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LegacyNachoPayment {
    pub id: i32,
    pub wallet_address: Vec<String>,
    pub nacho_amount: i64,
    pub timestamp: Option<NaiveDateTime>,
    pub transaction_hash: String,
}

/// One row of legacy `pending_krc20_transfers`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LegacyKrc20Transfer {
    pub id: i32,
    pub first_txn_id: String,
    pub sompi_to_miner: i64,
    pub nacho_amount: i64,
    pub address: String,
    pub p2sh_address: String,
    pub nacho_transfer_status: Option<String>,
    pub db_entry_status: Option<String>,
    pub timestamp: Option<NaiveDateTime>,
}

/// Total number of `block_details` rows. Used for progress
/// reporting + reconciliation.
pub async fn count_block_details(pool: &sqlx::PgPool) -> Result<i64, sqlx::Error> {
    let n: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM block_details")
        .fetch_one(pool)
        .await?;
    Ok(n)
}

/// Row count for any of the other legacy tables the importer reads.
pub async fn count(pool: &sqlx::PgPool, table: &str) -> Result<i64, sqlx::Error> {
    // table is a small whitelisted set of static strings from
    // importer code; safe to interpolate.
    let sql = format!("SELECT count(*)::bigint FROM {table}");
    let n: i64 = sqlx::query_scalar(&sql).fetch_one(pool).await?;
    Ok(n)
}

/// Sum a numeric column on any legacy table. Used by the
/// reconciliation phase to cross-check `sum(legacy) == sum(new)`.
pub async fn sum_bigint(
    pool: &sqlx::PgPool,
    table: &str,
    column: &str,
) -> Result<i64, sqlx::Error> {
    let sql = format!("SELECT COALESCE(sum({column}), 0)::bigint FROM {table}");
    let n: i64 = sqlx::query_scalar(&sql).fetch_one(pool).await?;
    Ok(n)
}

/// Sum a `numeric` column on any legacy table.
///
/// Returns a `BigDecimal` for columns typed `numeric` (where the
/// value may exceed `i64::MAX` in theory; in practice it never
/// has, but we preserve precision through the reconciliation step
/// regardless).
pub async fn sum_numeric(
    pool: &sqlx::PgPool,
    table: &str,
    column: &str,
) -> Result<sqlx::types::BigDecimal, sqlx::Error> {
    let sql = format!("SELECT COALESCE(sum({column}), 0)::numeric FROM {table}");
    let n: sqlx::types::BigDecimal = sqlx::query_scalar(&sql).fetch_one(pool).await?;
    Ok(n)
}

/// Stream every `miners_balance` row in one shot (table is small —
/// ~2,600 rows in production).
pub async fn fetch_miners_balance(
    pool: &sqlx::PgPool,
) -> Result<Vec<LegacyMinersBalance>, sqlx::Error> {
    sqlx::query_as::<_, LegacyMinersBalance>(
        "SELECT id, miner_id, wallet, balance, nacho_rebate_kas FROM miners_balance",
    )
    .fetch_all(pool)
    .await
}

/// Stream every `payments` row, ordered by `id` for a deterministic
/// import. Used by the payments transform.
pub async fn fetch_payments(pool: &sqlx::PgPool) -> Result<Vec<LegacyPayment>, sqlx::Error> {
    sqlx::query_as::<_, LegacyPayment>(
        "SELECT id, wallet_address, amount, timestamp, transaction_hash
           FROM payments ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
}

/// Stream every `nacho_payments` row, ordered by `id`.
pub async fn fetch_nacho_payments(
    pool: &sqlx::PgPool,
) -> Result<Vec<LegacyNachoPayment>, sqlx::Error> {
    sqlx::query_as::<_, LegacyNachoPayment>(
        "SELECT id, wallet_address, nacho_amount, timestamp, transaction_hash
           FROM nacho_payments ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
}

/// Stream every `pending_krc20_transfers` row, ordered by `id`.
pub async fn fetch_krc20_transfers(
    pool: &sqlx::PgPool,
) -> Result<Vec<LegacyKrc20Transfer>, sqlx::Error> {
    sqlx::query_as::<_, LegacyKrc20Transfer>(
        "SELECT id, first_txn_id, sompi_to_miner, nacho_amount, address, p2sh_address,
                nacho_transfer_status::text, db_entry_status::text, timestamp
           FROM pending_krc20_transfers ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
}

/// Stream a page of `block_details` ordered by `timestamp ASC`. The
/// importer walks the whole table by passing successive `after`
/// values, so we can resume on interruption.
pub async fn page_block_details(
    pool: &sqlx::PgPool,
    after_timestamp: Option<NaiveDateTime>,
    after_hash: Option<&str>,
    limit: i64,
) -> Result<Vec<LegacyBlockDetail>, sqlx::Error> {
    // Compound cursor (timestamp, mined_block_hash) gives a stable
    // ordering even for ties on timestamp.
    let rows = match (after_timestamp, after_hash) {
        (Some(t), Some(h)) => {
            sqlx::query_as::<_, LegacyBlockDetail>(
                "SELECT mined_block_hash, miner_id, pool_address, wallet,
                        daa_score, timestamp, reward_block_hash, miner_reward
                   FROM block_details
                  WHERE (timestamp, mined_block_hash) > ($1, $2)
                  ORDER BY timestamp ASC, mined_block_hash ASC
                  LIMIT $3",
            )
            .bind(t)
            .bind(h)
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
        _ => {
            sqlx::query_as::<_, LegacyBlockDetail>(
                "SELECT mined_block_hash, miner_id, pool_address, wallet,
                        daa_score, timestamp, reward_block_hash, miner_reward
                   FROM block_details
                  ORDER BY timestamp ASC, mined_block_hash ASC
                  LIMIT $1",
            )
            .bind(limit)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows)
}
