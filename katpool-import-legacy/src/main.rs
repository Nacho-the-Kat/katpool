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
use katpool_import_legacy::reconcile;
use katpool_import_legacy::transform::{
    TransformStats, balances, blocks, krc20, nacho_payments, payments,
};
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

    // Order matters: blocks first (creates wallet + worker rows that
    // every later transform reads via `wallet::ensure`), then the
    // independent transforms. Each is idempotent so a partial-failure
    // restart re-runs from the beginning safely.
    let blocks_stats = blocks::run(&source, &target, args.dry_run)
        .await
        .context("transform: block_details")?;
    info!(transform = "blocks", stats = %blocks_stats, "blocks transform done");
    totals = totals.add(&blocks_stats);

    let balances_stats = balances::run(&source, &target, args.dry_run)
        .await
        .context("transform: miners_balance")?;
    info!(transform = "balances", stats = %balances_stats, "balances transform done");
    totals = totals.add(&balances_stats);

    let payments_stats = payments::run(&source, &target, args.dry_run)
        .await
        .context("transform: payments")?;
    info!(transform = "payments", stats = %payments_stats, "payments transform done");
    totals = totals.add(&payments_stats);

    let nacho_stats = nacho_payments::run(&source, &target, args.dry_run)
        .await
        .context("transform: nacho_payments")?;
    info!(transform = "nacho_payments", stats = %nacho_stats, "nacho_payments transform done");
    totals = totals.add(&nacho_stats);

    let krc20_stats = krc20::run(&source, &target, args.dry_run)
        .await
        .context("transform: pending_krc20_transfers")?;
    info!(transform = "krc20", stats = %krc20_stats, "krc20 transform done");
    totals = totals.add(&krc20_stats);

    // Reconciliation pass is read-only and runs even in dry-run
    // mode (so operators see "this is what cutover would prove").
    let reconcile_report = reconcile::run(&source, &target)
        .await
        .context("reconcile")?;

    let elapsed = started.elapsed();
    info!(elapsed_secs = elapsed.as_secs_f64(), totals = %totals, all_passed = reconcile_report.all_passed, "importer complete");

    let report = serde_json::json!({
        "version": katpool_import_legacy::VERSION,
        "dry_run": args.dry_run,
        "elapsed_secs": elapsed.as_secs_f64(),
        "transforms": {
            "blocks":          stats_to_json(&blocks_stats),
            "balances":        stats_to_json(&balances_stats),
            "payments":        stats_to_json(&payments_stats),
            "nacho_payments":  stats_to_json(&nacho_stats),
            "krc20":           stats_to_json(&krc20_stats),
        },
        "totals": stats_to_json(&totals),
        "reconcile": reconcile_report,
    });
    println!("{report}");

    if !reconcile_report.all_passed {
        // Non-zero exit so a CI / runbook script catches the
        // mismatch even if it doesn't parse stdout.
        std::process::exit(2);
    }

    Ok(())
}

fn stats_to_json(s: &TransformStats) -> serde_json::Value {
    serde_json::json!({
        "read": s.read,
        "inserted": s.inserted,
        "skipped": s.skipped,
        "rejected": s.rejected,
    })
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .init();
}
