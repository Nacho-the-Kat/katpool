//! KRC-20 NACHO payout dry-run rehearsal binary.
//!
//! Drives exactly one **dry-run** NACHO payout cycle through the production
//! engine ([`payout_krc20::Krc20PayoutEngine`] in
//! [`payout_kas::ExecutionMode::DryRun`]): it acquires the single-leader
//! advisory lock, derives the DAA cycle window, quotes the NACHO floor price,
//! plans the eligible rebates into commit/reveal transfers, then mass-plans,
//! signs, and verifies each commit against the **live** treasury UTXO set
//! before reconciling — **without recording a txid, broadcasting, or
//! crediting any `nacho_rebate`**.
//!
//! Output is a single JSON envelope on stdout (the reconcile evidence) plus
//! structured `tracing` on stderr, mirroring `katpool-payout-rehearsal`. The
//! wrapper `scripts/krc20-payout-rehearsal.sh` captures both plus a manifest.
//!
//! ## Exit codes
//! - `0` — dry-run cycle planned cleanly (every selected transfer mass-planned
//!   and signed against the live treasury UTXO set with no error).
//! - `2` — planned, but at least one transfer could not be mass-planned/signed
//!   (`settle.errors` non-empty); e.g. the treasury is underfunded or a
//!   commit/reveal exceeds the mass limit. Investigate before a live run.
//! - `3` — another instance holds the payout leader lock; nothing was done.
//! - other — a hard failure (connect, key load, kaspad RPC, floor-price quote).

// stdout is this tool's primary deliverable (the reconcile JSON), so we opt out
// of the workspace `print_stdout = deny` default as the KAS rehearsal does.
#![allow(clippy::print_stdout)]

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use kaspa_addresses::{Address, Prefix};
use katpool_db::repo::{audit, payout};
use katpool_db::{PoolConfig, build_pool};
use katpool_krc20_rehearsal::{ENVELOPE_SCHEMA, RehearsalEvidence, RehearsalParams, VERSION};
use katpool_secrets::load_from_path;
use payout_kas::{ExecutionMode, GrpcKaspadClient, TREASURY_SPEND_LOCK_NAMESPACE};
use payout_krc20::{
    BreakeredSource, CircuitBreaker, DEFAULT_COMMIT_AMOUNT_SOMPI, DEFAULT_CYCLE_LIMIT,
    DEFAULT_HTTP_TIMEOUT, DEFAULT_MIN_NACHO_BASE_UNITS, DEFAULT_MIN_PENDING_SOMPI,
    DEFAULT_QUOTE_BASE, DEFAULT_QUOTE_TICKER, KaspaComFloorPrice, Krc20PayoutEngine,
    Krc20PayoutEngineConfig, Krc20TickOutcome,
};
use serde_json::json;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Drive one dry-run KRC-20 NACHO payout cycle and emit reconcile evidence (no broadcast)."
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

    /// Token ticker to quote and inscribe.
    #[arg(long, env = "KATPOOL_KRC20_TICKER", default_value = DEFAULT_QUOTE_TICKER)]
    ticker: String,

    /// Floor-price API base URL.
    #[arg(long, env = "KATPOOL_KRC20_QUOTE_BASE", default_value = DEFAULT_QUOTE_BASE)]
    quote_base: String,

    /// Minimum pending KAS-sompi for a wallet to be selected (coarse filter).
    #[arg(long, env = "KATPOOL_KRC20_MIN_PENDING_SOMPI", default_value_t = DEFAULT_MIN_PENDING_SOMPI)]
    min_pending_sompi: i64,

    /// Minimum converted NACHO base units worth a reveal (dust gate).
    #[arg(long, env = "KATPOOL_KRC20_MIN_NACHO_BASE_UNITS", default_value_t = DEFAULT_MIN_NACHO_BASE_UNITS)]
    min_nacho_base_units: u128,

    /// KAS-sompi locked into each commit P2SH output.
    #[arg(long, env = "KATPOOL_KRC20_COMMIT_AMOUNT_SOMPI", default_value_t = DEFAULT_COMMIT_AMOUNT_SOMPI)]
    commit_amount_sompi: u64,

    /// DAA width of the payout cycle window (must exceed the confirmation depth).
    #[arg(
        long,
        env = "KATPOOL_KRC20_PAYOUT_CYCLE_SPAN_DAA",
        default_value_t = 86_400
    )]
    cycle_span_daa: u64,

    /// Cap on recipients planned and transfers settled this tick.
    #[arg(long, env = "KATPOOL_KRC20_BATCH_LIMIT", default_value_t = DEFAULT_CYCLE_LIMIT)]
    batch_limit: i64,

    /// Instance label for the engine + advisory lock.
    #[arg(
        long,
        env = "KATPOOL_INSTANCE_ID",
        default_value = "katpool-krc20-rehearsal"
    )]
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
    info!(instance = %args.instance_id, "katpool-krc20-rehearsal starting (dry-run)");

    let pool = build_pool(&PoolConfig {
        min_connections: 1,
        max_connections: 4,
        application_name: "katpool-krc20-rehearsal".to_owned(),
        ..PoolConfig::production(args.database_url.clone())
    })
    .await
    .context("opening Postgres pool")?;

    // Pre-plan snapshot of who would be paid at the threshold.
    let eligible =
        payout::list_krc20_eligible_wallets(&pool, args.min_pending_sompi, args.batch_limit)
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

    // Real production floor-price source, fail-closed behind a circuit breaker.
    let quote = BreakeredSource::new(
        KaspaComFloorPrice::new(args.quote_base.clone(), DEFAULT_HTTP_TIMEOUT)
            .context("building NACHO floor-price client")?,
        CircuitBreaker::new(3, Duration::from_secs(60)),
    );

    let engine = Krc20PayoutEngine::new(
        pool.clone(),
        client,
        secret,
        address.clone(),
        quote,
        Krc20PayoutEngineConfig {
            instance_id: args.instance_id.clone(),
            // Unused by run_once; only run_loop schedules on it.
            poll_interval: Duration::from_secs(60),
            cycle_span_daa: args.cycle_span_daa,
            mode: ExecutionMode::DryRun,
            lock_namespace: TREASURY_SPEND_LOCK_NAMESPACE.to_owned(),
            min_pending_sompi: args.min_pending_sompi,
            min_nacho_base_units: args.min_nacho_base_units,
            ticker: args.ticker.clone(),
            commit_amount_sompi: args.commit_amount_sompi,
            batch_limit: args.batch_limit,
        },
    )
    .context("building krc20 payout engine")?;

    let outcome = engine.run_once().await.context("running dry-run cycle")?;
    let report = match outcome {
        Krc20TickOutcome::Ran(report) => report,
        Krc20TickOutcome::SkippedNotLeader => {
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
    let transfers = payout::list_krc20_for_cycle(&pool, report.cycle_id)
        .await
        .context("loading planned transfers")?;
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
        ticker: args.ticker.clone(),
        min_pending_sompi: args.min_pending_sompi,
        min_nacho_base_units: args.min_nacho_base_units,
        commit_amount_sompi: args.commit_amount_sompi,
        cycle_span_daa: args.cycle_span_daa,
        virtual_daa: report.virtual_daa,
    };
    let evidence = RehearsalEvidence {
        params: &params,
        eligible: &eligible,
        cycle: &cycle,
        transfers: &transfers,
        payouts: &payouts,
        settle: &report.settle,
        credit: &report.credit,
        reconciled_status: report.status,
        audit: &audit,
    };
    let envelope = evidence.to_envelope();

    info!(
        cycle_id = report.cycle_id,
        eligible = eligible.len(),
        transfers = transfers.len(),
        settle_errors = report.settle.errors.len(),
        "dry-run cycle complete"
    );
    println!("{envelope}");

    // Go/no-go: a clean dry-run mass-plans and signs every selected transfer
    // against the live treasury UTXO set with no error.
    if !report.settle.errors.is_empty() {
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
