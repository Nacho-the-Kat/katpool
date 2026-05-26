//! Legacy importer binary entry point.
//!
//! Two `DATABASE_URL`-style arguments: `--source-url` for the legacy
//! `katpool_mainnet` (read-only) and `--target-url` for the new
//! schema (read-write). The importer applies migrations to the
//! target before running any transform, so a fresh target DB is a
//! valid starting state.
//!
//! Output is a single JSON line on stdout summarising the
//! reconciliation, plus structured `tracing` events on stderr. The
//! JSON output is meant for piping into the cutover runbook's
//! evidence collection.

// stdout output is the binary's primary deliverable — the workspace
// `print_stdout = deny` lint is a default for service crates; we
// explicitly opt out for this importer binary where the JSON
// reconciliation report is the contract with the operator.
#![allow(clippy::print_stdout)]

use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use katpool_db::{PoolConfig, build_pool};
use katpool_import_legacy::transform::{TransformStats, blocks};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Migrate the legacy katpool_mainnet database into the new schema."
)]
struct Args {
    /// Connection string for the legacy source database
    /// (`postgres://user:pass@host:port/katpool_mainnet`).
    #[arg(long, env = "KATPOOL_IMPORT_SOURCE_URL")]
    source_url: String,

    /// Connection string for the target (new-schema) database.
    #[arg(long, env = "KATPOOL_IMPORT_TARGET_URL")]
    target_url: String,

    /// Run every transform in dry-run mode (no target writes).
    /// Reconciliation counts still produced.
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Skip running migrations on the target. Useful when the target
    /// is a snapshot that already has the schema applied.
    #[arg(long, default_value_t = false)]
    skip_migrate: bool,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    init_tracing();

    info!(dry_run = args.dry_run, "katpool-import-legacy starting");

    // Source pool — small, read-only workload.
    let source_cfg = PoolConfig {
        url: args.source_url.clone(),
        min_connections: 1,
        max_connections: 4,
        application_name: "katpool-import-legacy[source]".to_owned(),
        ..PoolConfig::production(args.source_url.clone())
    };
    let source = build_pool(&source_cfg).await.context("connect source")?;

    // Target pool — modest writes; default sizing is generous enough.
    let target_cfg = PoolConfig {
        url: args.target_url.clone(),
        min_connections: 2,
        max_connections: 8,
        application_name: "katpool-import-legacy[target]".to_owned(),
        ..PoolConfig::production(args.target_url.clone())
    };
    let target = build_pool(&target_cfg).await.context("connect target")?;

    if !args.skip_migrate {
        katpool_db::migrate::run(&target)
            .await
            .context("apply target migrations")?;
        info!("target migrations applied");
    }

    let started = Instant::now();
    let mut totals = TransformStats::default();

    // ----- blocks (PR A) ---------------------------------------------
    let blocks_stats = blocks::run(&source, &target, args.dry_run)
        .await
        .context("transform: block_details")?;
    info!(transform = "blocks", stats = %blocks_stats, "blocks transform done");
    totals = totals.add(&blocks_stats);

    // ----- (future PR B): miners_balance, payments, nacho_payments,
    // pending_krc20_transfers transforms wired here. -----------------

    let elapsed = started.elapsed();
    info!(elapsed_secs = elapsed.as_secs_f64(), totals = %totals, "importer complete");

    let report = serde_json::json!({
        "version": katpool_import_legacy::VERSION,
        "dry_run": args.dry_run,
        "elapsed_secs": elapsed.as_secs_f64(),
        "transforms": {
            "blocks": {
                "read": blocks_stats.read,
                "inserted": blocks_stats.inserted,
                "skipped": blocks_stats.skipped,
                "rejected": blocks_stats.rejected,
            },
        },
        "totals": {
            "read": totals.read,
            "inserted": totals.inserted,
            "skipped": totals.skipped,
            "rejected": totals.rejected,
        },
    });
    println!("{report}");

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .init();
}
