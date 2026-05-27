//! `katpool-replay` — feed captured events through the accountant.
//!
//! Dual-database determinism verification is exercised in CI via
//! `accountant/tests/replay_harness_scale.rs` and operator rehearsal
//! via `scripts/replay-determinism-rehearsal.sh`.
//!
//! See `docs/runbooks/17-replay-determinism.md`.

#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use katpool_db::{PoolConfig, build_pool, migrate};
use katpool_replay::{load_ndjson_path, legacy_log, replay_all, snapshot};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Replay PoolEvent logs through the accountant."
)]
struct Args {
    /// Postgres URL (must have the katpool schema migrated).
    #[arg(long, env = "KATPOOL_DATABASE_URL")]
    database_url: String,

    /// Schema network for `wallet::ensure`.
    #[arg(long, env = "KATPOOL_NETWORK", default_value = "mainnet")]
    network: String,

    #[arg(long, env = "KATPOOL_INSTANCE_ID", default_value = "katpool-replay")]
    instance_id: String,

    /// Canonical NDJSON `PoolEvent` log (one JSON object per line).
    #[arg(long, conflicts_with = "legacy_log")]
    events: Option<PathBuf>,

    /// Legacy `katpool-app` monitoring log (share DEBUG lines).
    #[arg(long, conflicts_with = "events")]
    legacy_log: Option<PathBuf>,

    /// Keep every Nth event (1 = all). Use 50 for 1:50 subsampling.
    #[arg(long, default_value_t = 1)]
    subsample_nth: u64,

    /// Emit a JSON summary envelope on stdout after replay.
    #[arg(long)]
    emit_summary: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let started = Instant::now();

    let (input_kind, mut events) = if let Some(path) = &args.events {
        (
            "ndjson",
            load_ndjson_path(path).with_context(|| format!("loading NDJSON from {}", path.display()))?,
        )
    } else if let Some(path) = &args.legacy_log {
        let report = legacy_log::parse_legacy_log_path(path)
            .with_context(|| format!("parsing legacy log {}", path.display()))?;
        info!(
            lines = report.stats.lines_read,
            emitted = report.stats.events_emitted,
            credited = report.stats.share_credited,
            rejected = report.stats.share_rejected,
            unmatched = report.stats.lines_unmatched,
            "legacy log parsed"
        );
        ("legacy_monitoring", report.events)
    } else {
        anyhow::bail!("one of --events or --legacy-log is required");
    };

    if args.subsample_nth > 1 {
        let before = events.len();
        events = legacy_log::subsample_every_nth(events, args.subsample_nth);
        info!(
            nth = args.subsample_nth,
            before,
            after = events.len(),
            "subsampled event stream"
        );
    }

    if events.is_empty() {
        anyhow::bail!("event stream is empty after parsing");
    }

    let db = build_pool(&PoolConfig {
        url: args.database_url.clone(),
        min_connections: 1,
        max_connections: 8,
        application_name: format!("katpool-replay[{}]", args.instance_id),
        ..PoolConfig::production("placeholder".to_owned())
    })
    .await
    .context("opening postgres pool")?;
    migrate::run(&db).await.context("migrate target schema")?;

    let cfg = accountant::ConsumerConfig::new(args.instance_id.clone(), args.network.clone())
        .context("consumer config")?;
    let consumer = accountant::EventConsumer::new(db.clone(), cfg);
    replay_all(&consumer, &events).await;
    let snap = snapshot(&db).await?;

    info!(
        wallets = snap.wallets.len(),
        workers = snap.workers.len(),
        shares = snap.shares.len(),
        rejects = snap.rejects.len(),
        blocks = snap.blocks.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "replay complete"
    );

    if args.emit_summary {
        let envelope = serde_json::json!({
            "schema": "katpool-replay.summary/v1",
            "input_kind": input_kind,
            "event_count": events.len(),
            "wallets": snap.wallets.len(),
            "workers": snap.workers.len(),
            "shares": snap.shares.len(),
            "rejects": snap.rejects.len(),
            "blocks": snap.blocks.len(),
            "elapsed_ms": started.elapsed().as_millis(),
        });
        println!("{}", serde_json::to_string(&envelope)?);
    }

    Ok(())
}
