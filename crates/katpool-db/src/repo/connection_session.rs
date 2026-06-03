//! Connection-session aggregate — per-stratum-TCP-connection record.
//!
//! Sessions are created at TCP-accept (often *before* the
//! `mining.authorize` payload reveals which worker is connecting),
//! so `worker_id` is nullable. The accountant fills it in on the
//! first `ShareCredited` event for that session.
//!
//! Used for per-IP forensics, per-rig analytics, and anti-abuse
//! audit trails.

// Sessions populated with shares_credited/_rejected/malformed_frames
// counters from the bridge's anti-abuse layer; those are u64-like
// counts but stored as BIGINT (signed). See the share/block modules
// for the same boundary rationale.
#![allow(clippy::cast_possible_wrap)]

use std::net::IpAddr;

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;

use crate::DbError;
use crate::repo::{SessionId, WorkerId};

/// One row of the `connection_session` table.
///
/// The `remote_ip` column is a postgres `INET` server-side. We map it
/// as `String` at the Rust boundary (canonical bech32-like text form)
/// rather than `std::net::IpAddr` because sqlx doesn't ship a built-in
/// `IpAddr` codec; using `String` avoids pulling in the `ipnetwork`
/// crate just for this one column. Postgres handles the text↔inet
/// cast both directions, so range queries on `inet` columns still work
/// from the application side.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConnectionSession {
    /// Synthetic primary key.
    pub id: SessionId,
    /// FK to `worker.id`; nullable for pre-authorize sessions.
    pub worker_id: Option<WorkerId>,
    /// Remote endpoint IP as text (postgres `INET` column).
    pub remote_ip: String,
    /// Stratum `mining.subscribe` user-agent string, if any.
    pub remote_app: Option<String>,
    /// TCP-accept timestamp.
    pub connected_at: DateTime<Utc>,
    /// Disconnect timestamp; `None` while still active.
    pub disconnected_at: Option<DateTime<Utc>>,
    /// Running count of accepted shares for this session.
    pub shares_credited: i64,
    /// Running count of rejected shares for this session.
    pub shares_rejected: i64,
    /// Running count of frames that failed JSON-RPC parsing.
    pub malformed_frames: i64,
}

/// Insert a fresh session at TCP-accept time. Returns the new id so
/// the bridge can carry it on the share-handler context.
pub async fn open<'e, E>(
    executor: E,
    remote_ip: IpAddr,
    remote_app: Option<&str>,
) -> Result<SessionId, DbError>
where
    E: PgExecutor<'e>,
{
    let id: SessionId = sqlx::query_scalar::<_, SessionId>(
        "INSERT INTO connection_session (remote_ip, remote_app)
         VALUES ($1::inet, $2)
         RETURNING id",
    )
    .bind(remote_ip.to_string())
    .bind(remote_app)
    .fetch_one(executor)
    .await?;
    Ok(id)
}

/// Insert an already-completed session row in a single statement.
///
/// Used by the accountant when it learns of a session only at
/// disconnect (the bridge holds no DB handle, so it cannot `open` at
/// accept time). `worker_id` is `None` for sessions that never
/// authorized. Per-session counters are left at their schema defaults
/// (0) — this path records identity + lifetime, not share tallies.
pub async fn record_closed<'e, E>(
    executor: E,
    worker_id: Option<WorkerId>,
    remote_ip: IpAddr,
    remote_app: Option<&str>,
    connected_at: DateTime<Utc>,
    disconnected_at: DateTime<Utc>,
) -> Result<SessionId, DbError>
where
    E: PgExecutor<'e>,
{
    let id: SessionId = sqlx::query_scalar::<_, SessionId>(
        "INSERT INTO connection_session
             (worker_id, remote_ip, remote_app, connected_at, disconnected_at)
         VALUES ($1, $2::inet, $3, $4, $5)
         RETURNING id",
    )
    .bind(worker_id.map(|w| w.0))
    .bind(remote_ip.to_string())
    .bind(remote_app)
    .bind(connected_at)
    .bind(disconnected_at)
    .fetch_one(executor)
    .await?;
    Ok(id)
}

/// Bind a worker to an already-open session.
///
/// Called by the accountant when the first `ShareCredited` event for
/// the session arrives — the session was created at TCP accept, but
/// the worker identity wasn't known until authorize.
pub async fn bind_worker<'e, E: PgExecutor<'e>>(
    executor: E,
    session_id: SessionId,
    worker_id: WorkerId,
) -> Result<(), DbError> {
    sqlx::query("UPDATE connection_session SET worker_id = $2 WHERE id = $1 AND worker_id IS NULL")
        .bind(session_id.0)
        .bind(worker_id.0)
        .execute(executor)
        .await?;
    Ok(())
}

/// Close the session at TCP-disconnect.
pub async fn close<'e, E: PgExecutor<'e>>(
    executor: E,
    session_id: SessionId,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE connection_session
            SET disconnected_at = COALESCE(disconnected_at, now())
          WHERE id = $1",
    )
    .bind(session_id.0)
    .execute(executor)
    .await?;
    Ok(())
}

/// Increment the per-session counters atomically. Called once per
/// `PoolEvent` the bridge surfaces.
pub async fn increment_counters<'e, E>(
    executor: E,
    session_id: SessionId,
    credited_delta: u32,
    rejected_delta: u32,
    malformed_delta: u32,
) -> Result<(), DbError>
where
    E: PgExecutor<'e>,
{
    sqlx::query(
        "UPDATE connection_session
            SET shares_credited  = shares_credited  + $2,
                shares_rejected  = shares_rejected  + $3,
                malformed_frames = malformed_frames + $4
          WHERE id = $1",
    )
    .bind(session_id.0)
    .bind(i64::from(credited_delta))
    .bind(i64::from(rejected_delta))
    .bind(i64::from(malformed_delta))
    .execute(executor)
    .await?;
    Ok(())
}

/// One row of the firmware / user-agent breakdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareCount {
    /// Reported stratum `mining.subscribe` user-agent, or `None` when the
    /// client sent none. Callers normalize this to a vendor for display.
    pub remote_app: Option<String>,
    /// Distinct workers reporting this user-agent in the window.
    pub workers: i64,
    /// Sessions opened with this user-agent in the window.
    pub sessions: i64,
}

/// Breakdown of distinct workers (and sessions) by reported stratum
/// user-agent, over sessions that overlapped `[since, now]`.
///
/// A session overlaps the window if it is still open
/// (`disconnected_at IS NULL`) or ended at/after `since`. `workers`
/// counts distinct non-null `worker_id` (pre-authorize sessions have a
/// null worker and contribute only to `sessions`). Drives the
/// dashboard's firmware/device breakdown. Ordered by descending
/// workers, then sessions, for a stable display.
pub async fn firmware_breakdown<'e, E>(
    executor: E,
    since: DateTime<Utc>,
) -> Result<Vec<FirmwareCount>, DbError>
where
    E: PgExecutor<'e>,
{
    let rows: Vec<(Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT remote_app,
                count(DISTINCT worker_id)::bigint AS workers,
                count(*)::bigint AS sessions
           FROM connection_session
          WHERE disconnected_at IS NULL
             OR disconnected_at >= $1
          GROUP BY remote_app
          ORDER BY workers DESC, sessions DESC",
    )
    .bind(since)
    .fetch_all(executor)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(remote_app, workers, sessions)| FirmwareCount {
            remote_app,
            workers,
            sessions,
        })
        .collect())
}

/// List recent sessions for a given worker, newest-first.
pub async fn list_for_worker<'e, E: PgExecutor<'e>>(
    executor: E,
    worker_id: WorkerId,
    limit: i64,
) -> Result<Vec<ConnectionSession>, DbError> {
    sqlx::query_as::<_, ConnectionSession>(
        "SELECT id, worker_id, host(remote_ip)::text AS remote_ip, remote_app,
                connected_at, disconnected_at,
                shares_credited, shares_rejected, malformed_frames
           FROM connection_session
          WHERE worker_id = $1
          ORDER BY connected_at DESC
          LIMIT $2",
    )
    .bind(worker_id.0)
    .bind(limit)
    .fetch_all(executor)
    .await
    .map_err(DbError::from)
}
