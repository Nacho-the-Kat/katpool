//! KAS payout dry-run rehearsal binary.
//!
//! Drives exactly one **dry-run** payout cycle through the production engine
//! ([`payout_kas::PayoutEngine`] in [`payout_kas::ExecutionMode::DryRun`]):
//! it acquires the single-leader advisory lock, derives the DAA cycle window,
//! plans against the live treasury UTXO set, signs + verifies every batch, then
//! reconciles — **without broadcasting and without marking any row submitted**.
//!
//! Output is a single JSON envelope on stdout (the reconcile evidence) plus
//! structured `tracing` on stderr, mirroring `katpool-import-legacy`. The
//! wrapper `scripts/kas-payout-rehearsal.sh` captures both plus a manifest.
//!
//! ## Exit codes
//! - `0` — dry-run cycle planned cleanly (every eligible recipient funded).
//! - `2` — planned, but the treasury could not fund everyone
//!   (`unpaid > 0`) or a signing/verification error occurred. Investigate
//!   before a live run.
//! - `3` — another instance holds the payout leader lock; nothing was done.
//! - other — a hard failure (connect, key load, kaspad RPC).

// stdout is this tool's primary deliverable (the reconcile JSON), so we opt out
// of the workspace `print_stdout = deny` default as the importer binary does.
#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use kaspa_addresses::{Address, Prefix};
use katpool_db::repo::{audit, payout};
use katpool_db::{PoolConfig, build_pool};
use katpool_payout_rehearsal::{ENVELOPE_SCHEMA, RehearsalEvidence, RehearsalParams, VERSION};
use katpool_secrets::load_from_path;
use payout_kas::{
    DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI, ExecutionMode, GrpcKaspadClient, PayoutEngine,
    PayoutEngineConfig, TREASURY_SPEND_LOCK_NAMESPACE, TickOutcome,
};
use serde_json::json;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Drive one dry-run KAS payout cycle and emit reconcile evidence (no broadcast)."
)]
struct Args {
    /// kaspad gRPC URL (`grpc://host:port`).
    #[arg(long, env = "KASPAD_GRPC_URL")]
    kaspad_url: String,

    /// Target (new-schema) Postgres URL.
    #[arg(long, env = "KATPOOL_DATABASE_URL")]
    database_url: String,

    /// Path to the raw 32-byte hex treasury key (testnet rehearsal). The file
    /// is read and its contents zeroized after load.
    #[arg(long, env = "KATPOOL_TREASURY_KEY_PATH")]
    treasury_key_path: PathBuf,

    /// Treasury (pool) address. If comma-separated, the first entry is used.
    #[arg(long, env = "KATPOOL_POOL_ADDRESS")]
    treasury_address: String,

    /// Schema-network label. Derived from the address prefix when unset.
    #[arg(long, env = "KATPOOL_NETWORK")]
    network: Option<String>,

    /// Eligibility threshold in sompi.
    #[arg(long, env = "KATPOOL_PAYOUT_THRESHOLD_SOMPI", default_value_t = DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI)]
    threshold_sompi: i64,

    /// DAA width of the payout cycle window (must exceed the confirmation depth).
    #[arg(long, env = "KATPOOL_PAYOUT_CYCLE_SPAN_DAA", default_value_t = 86_400)]
    cycle_span_daa: u64,

    /// Instance label for the engine + advisory lock.
    #[arg(long, env = "KATPOOL_INSTANCE_ID", default_value = "katpool-rehearsal")]
    instance_id: String,

    /// Maximum audit-log entries to attach to the envelope.
    #[arg(long, default_value_t = 200)]
    audit_limit: i64,
}

// One-shot orchestrator: connect, snapshot eligibility, plan one dry-run
// cycle, gather the persisted evidence, emit the envelope. Kept long-form
// for traceability rather than split across helpers that each run once.
#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    init_tracing();
    info!(instance = %args.instance_id, "katpool-payout-rehearsal starting (dry-run)");

    let pool = build_pool(&PoolConfig {
        url: args.database_url.clone(),
        min_connections: 1,
        max_connections: 4,
        application_name: "katpool-payout-rehearsal".to_owned(),
        ..PoolConfig::production(args.database_url.clone())
    })
    .await
    .context("opening Postgres pool")?;

    // Pre-plan snapshot of who would be paid at the threshold.
    let eligible = payout::list_kas_eligible_wallets(&pool, args.threshold_sompi)
        .await
        .context("listing eligible wallets")?;

    let address_str = args
        .treasury_address
        .split(',')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("KATPOOL_POOL_ADDRESS produced an empty list"))?;
    let address = Address::try_from(address_str)
        .map_err(|e| anyhow!("treasury address `{address_str}`: {e}"))?;
    let network = args
        .network
        .clone()
        .unwrap_or_else(|| derive_network(address.prefix).to_owned());

    let secret = load_from_path(&args.treasury_key_path).with_context(|| {
        format!(
            "loading treasury key from {}",
            args.treasury_key_path.display()
        )
    })?;

    let client = GrpcKaspadClient::connect(args.kaspad_url.clone())
        .await
        .context("connecting to kaspad gRPC")?;

    let engine = PayoutEngine::new(
        pool.clone(),
        client,
        secret,
        address.clone(),
        PayoutEngineConfig {
            instance_id: args.instance_id.clone(),
            // Unused by run_once; only run_loop schedules on it.
            poll_interval: Duration::from_secs(60),
            cycle_span_daa: args.cycle_span_daa,
            threshold_sompi: args.threshold_sompi,
            // Rehearsal is dry-run only; the spend cap is a live-broadcast guard.
            max_payout_sompi_per_cycle: None,
            mode: ExecutionMode::DryRun,
            lock_namespace: TREASURY_SPEND_LOCK_NAMESPACE.to_owned(),
        },
    )
    .context("building payout engine")?;

    let outcome = engine.run_once().await.context("running dry-run cycle")?;
    let report = match outcome {
        TickOutcome::Ran(report) => report,
        TickOutcome::SkippedNotLeader => {
            // Another leader holds the lock; do not pretend a cycle ran.
            let envelope = json!({
                "schema": ENVELOPE_SCHEMA,
                "version": VERSION,
                "dry_run": true,
                "error": "skipped_not_leader",
                "detail": "another instance holds the payout leader lock; stop it or wait, then retry",
            });
            println!("{envelope}");
            std::process::exit(3);
        }
    };

    // Gather the persisted evidence for the planned cycle.
    let cycle = payout::get_cycle(&pool, report.cycle_id)
        .await
        .context("loading planned cycle")?;
    let payouts = payout::list_for_cycle(&pool, report.cycle_id)
        .await
        .context("loading planned payouts")?;
    let audit = audit::list_for_subject(&pool, "payout_cycle", report.cycle_id, args.audit_limit)
        .await
        .context("loading cycle audit trail")?;

    let params = RehearsalParams {
        instance_id: args.instance_id.clone(),
        network,
        treasury_address: address.to_string(),
        threshold_sompi: args.threshold_sompi,
        cycle_span_daa: args.cycle_span_daa,
        virtual_daa: report.virtual_daa,
    };
    let evidence = RehearsalEvidence {
        params: &params,
        eligible: &eligible,
        cycle: &cycle,
        payouts: &payouts,
        broadcast: &report.broadcast,
        confirm: &report.confirm,
        reconciled_status: report.status,
        audit: &audit,
    };
    let envelope = evidence.to_envelope();

    info!(
        cycle_id = report.cycle_id,
        eligible = eligible.len(),
        planned_batches = report.broadcast.planned_batches,
        unpaid = report.broadcast.unpaid,
        "dry-run cycle complete"
    );
    println!("{envelope}");

    // Go/no-go: a clean dry-run funds every eligible recipient with no errors.
    if !report.broadcast.submit_errors.is_empty() || report.broadcast.unpaid > 0 {
        std::process::exit(2);
    }
    Ok(())
}

const fn derive_network(prefix: Prefix) -> &'static str {
    match prefix {
        Prefix::Mainnet => "mainnet",
        Prefix::Testnet => "testnet-10",
        Prefix::Devnet => "devnet",
        Prefix::Simnet => "simnet",
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true)
        .init();
}
