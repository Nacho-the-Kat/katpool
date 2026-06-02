//! katpool — Phase 7 wiring binary.
//!
//! As of Phase 3 M3d the binary composes the **stratum bridge**
//! plus the **accountant**'s event consumer and maturity tracker
//! into a single process with a shared `broadcast::Sender<PoolEvent>`
//! channel. Phase 4 adds payout-kas, Phase 5 payout-krc20,
//! Phase 6 the read-only API, Phase 7 closes out telemetry/
//! secrets/config wiring.
//!
//! ## Subsystems
//!
//! 1. **Bridge stratum server**. Listens on `KATPOOL_STRATUM_PORT`.
//!    Talks to kaspad via the bridge's own `KaspaApi` (block
//!    template fetch + submission). Emits `PoolEvent` into the
//!    shared broadcast channel.
//! 2. **Accountant event consumer**. Drains the broadcast channel,
//!    writes share / block rows via the new schema's repo layer.
//! 3. **Maturity tracker**. Polls kaspad (via the same gRPC URL,
//!    separate client): resolves `submitted_to_node` blocks to
//!    `confirmed_blue` / `orphaned` by GHOSTDAG colour, and allocates
//!    matured coinbase UTXOs credited to the pool address via the
//!    accountant's allocation engine.
//!
//! All three subsystems shut down cleanly on SIGINT / SIGTERM via
//! a `tokio::sync::watch::Receiver<bool>` propagated from the
//! signal task.
//!
//! ## Commands
//!
//! Invoked with no arguments the binary runs the full daemon above. It also
//! accepts an operator on-demand payout subcommand:
//!
//! - `katpool payout run-now [--dry-run]` — drive a single KAS payout cycle
//!   synchronously (plan → broadcast → confirm → reconcile), then exit. It
//!   reads the same environment configuration as the daemon (including
//!   `KATPOOL_PAYOUT_DRY_RUN`) and acquires the shared `payout-kas:kas-leader`
//!   advisory lock, so it is safe to run while the daemon is live — only one
//!   cycle driver acts at a time. `--dry-run` forces sign+verify without
//!   broadcasting regardless of the env setting.
//! - `katpool --help` — print usage.
//!
//! ## Configuration (env-var only in M3d; YAML in Phase 7)
//!
//! Required:
//! - `KASPAD_GRPC_URL`               (e.g. `grpc://127.0.0.1:16210`)
//! - `KATPOOL_DATABASE_URL`          postgres URL
//! - `KATPOOL_POOL_ADDRESS`          kaspa address(es), comma-separated
//!   (coinbase outputs to these become pool revenue)
//! - `KATPOOL_STRATUM_PORT`          e.g. `5555`
//!
//! Optional:
//! - `KATPOOL_INSTANCE_ID`           default `katpool-runtime`
//! - `KATPOOL_FEE_TOPLINE_BPS`       default 75
//! - `KATPOOL_MIN_SHARE_DIFF`        default 4096 (ASIC-class floor;
//!   raise for higher-hashrate fleets, lower only for CPU/dev miners)
//! - `KATPOOL_VAR_DIFF`              default `true` (variable difficulty
//!   retargeting; set `false` to pin every miner at `min_share_diff`)
//! - `KATPOOL_SHARES_PER_MIN`        default 20 (vardiff retarget setpoint;
//!   ignored when `KATPOOL_VAR_DIFF=false`)
//! - `KATPOOL_PROM_PORT`             default empty (disabled)
//! - `KATPOOL_HEALTH_CHECK_PORT`     **no-op in the unified runtime** — it is
//!   carried into `BridgeServerConfig.health_check_port` for the standalone
//!   bridge binary but `listen_and_serve_with_events` never serves it here.
//!   Liveness/readiness come from the API on `KATPOOL_API_PORT` (ADR-0021).
//! - `KATPOOL_MATURITY_POLL_SECS`    default 15
//! - `KATPOOL_COINBASE_MATURITY`     default 1000 (DAA-score depth)
//! - `KATPOOL_WINDOW_DAA_SPAN`       default 600
//! - `KATPOOL_BROADCAST_CAPACITY`    default 4096
//! - `KATPOOL_EVENT_RECORD_PATH`     optional NDJSON `PoolEvent` capture
//!   for M4 replay-determinism rehearsal
//!
//! Public read-only HTTP API (Phase 6 — opt-in, ADR-0021):
//! - `KATPOOL_API_PORT`              bind address `host:port` (e.g.
//!   `127.0.0.1:8080`); empty = disabled. Serves the unversioned `/health`
//!   `/ready` `/started` probes plus the versioned `/api/v1` read-only data
//!   surface. `/ready` is DB-reachable AND kaspad-synced; the kaspad-sync
//!   signal reuses the maturity tracker's existing poll (no second gRPC
//!   connection), and `/started` latches once the first sweep observes it.
//! - `KATPOOL_API_RATE_PER_SECOND`   default 5  (per-IP sustained refill)
//! - `KATPOOL_API_RATE_BURST`        default 20 (per-IP burst capacity)
//! - `KATPOOL_API_REQUEST_TIMEOUT_SECS`  default 5
//! - `KATPOOL_API_POOL_CACHE_TTL_SECS`   default 10
//! - `KATPOOL_API_WALLET_CACHE_TTL_SECS` default 5
//! - `KATPOOL_API_CORS_ALLOW_ORIGIN` default empty (no CORS layer installed)
//!
//! KAS payout engine (M4.7 — opt-in, dry-run by default):
//! - `KATPOOL_PAYOUT_ENABLED`        default `false` (engine off)
//! - `KATPOOL_PAYOUT_DRY_RUN`        default `true` (sign+verify only;
//!   set `false` to broadcast real transactions)
//! - `KATPOOL_PAYOUT_POLL_SECS`      default 60
//! - `KATPOOL_PAYOUT_CYCLE_SPAN_DAA` default `216_000` (~6h at 10 BPS;
//!   block-rate-specific, must exceed the confirmation depth)
//! - `KATPOOL_PAYOUT_THRESHOLD_SOMPI` default 10 KAS
//! - Treasury key source (one of, in precedence order):
//!   `KATPOOL_TREASURY_KEY_PATH` (raw 32-byte hex file, testnet
//!   rehearsal) else `KATPOOL_TREASURY_CREDENTIAL` (systemd
//!   `LoadCredentialEncrypted` name, default `treasury-key`).
//!   The treasury address is the first `KATPOOL_POOL_ADDRESS`.
//!
//! KRC-20 NACHO payout engine (M5.5b — opt-in, dry-run by default; shares
//! the treasury key/address and kaspad node, separate advisory-lock leader):
//! - `KATPOOL_KRC20_PAYOUT_ENABLED`        default `false` (engine off)
//! - `KATPOOL_KRC20_PAYOUT_DRY_RUN`        default `true` (settle records +
//!   broadcasts nothing; never credits)
//! - `KATPOOL_KRC20_PAYOUT_POLL_SECS`      default 60
//! - `KATPOOL_KRC20_PAYOUT_CYCLE_SPAN_DAA` default `216_000` (~6h at 10 BPS;
//!   block-rate-specific, must exceed the confirmation depth)
//! - `KATPOOL_KRC20_MIN_PENDING_SOMPI`     default 10 KAS (coarse pre-filter)
//! - `KATPOOL_KRC20_MIN_NACHO_BASE_UNITS`  default 1 NACHO (dust gate)
//! - `KATPOOL_KRC20_COMMIT_AMOUNT_SOMPI`   default 0.2 KAS (commit P2SH lock)
//! - commit/reveal network fees are sized adaptively from the node fee-rate
//!   (floored at the relay minimum) and frozen per-transfer; not configurable
//! - `KATPOOL_KRC20_BATCH_LIMIT`           default 1000 recipients/tick
//! - `KATPOOL_KRC20_TICKER`                default `NACHO`
//! - `KATPOOL_KRC20_QUOTE_BASE`            default `https://api.kaspa.com`
//! - `KATPOOL_KRC20_QUOTE_BREAKER_THRESHOLD` default 3 consecutive failures
//! - `KATPOOL_KRC20_QUOTE_BREAKER_COOLDOWN_SECS` default 60

#![cfg_attr(not(test), warn(missing_docs))]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use api::{ApiConfig, AppState, ReadinessHandle};
use kaspa_addresses::{Address, Prefix};
use kaspa_grpc_client::GrpcClient;
use kaspa_rpc_core::notify::mode::NotificationMode;
use kaspa_stratum_bridge::{
    BridgeConfig as BridgeServerConfig, KaspaApi, listen_and_serve_with_events, prom,
};
use katpool_db::{PoolConfig, build_pool};
use katpool_domain::PoolEvent;
use katpool_secrets::{load_from_path, load_from_systemd_credential};
use payout_kas::{
    DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI, ExecutionMode, GrpcKaspadClient, PayoutEngine,
    PayoutEngineConfig, TickOutcome,
};
use payout_krc20::{
    BreakeredSource, CircuitBreaker, DEFAULT_COMMIT_AMOUNT_SOMPI, DEFAULT_CYCLE_LIMIT,
    DEFAULT_HTTP_TIMEOUT, DEFAULT_MIN_NACHO_BASE_UNITS, DEFAULT_MIN_PENDING_SOMPI,
    DEFAULT_QUOTE_BASE, DEFAULT_QUOTE_TICKER, KaspaComFloorPrice, Krc20PayoutEngine,
    Krc20PayoutEngineConfig,
};
use tokio::io::AsyncWriteExt;
use tokio::signal;
use tokio::sync::{broadcast, watch};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use accountant::{
    AllocationEngine, ConsumerConfig, EventConsumer, FeeConfig, KaspadGrpcClient, MaturityConfig,
    MaturityTracker, StaticTierClassifier,
};

// The runtime orchestrator is intentionally long-form: every step
// is a single named operation against the workspace's subsystems,
// and abstracting them out reduces traceability for a path that
// composes multiple critical lifecycles.
#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(true)
        .init();

    let arg_list: Vec<String> = std::env::args().skip(1).collect();
    let command = parse_args(&arg_list).context("parsing arguments")?;
    if command == Command::Help {
        print_usage();
        return Ok(());
    }

    let cfg = RuntimeConfig::from_env().context("loading runtime config")?;

    // Operator on-demand payout: drive one cycle synchronously and exit,
    // never starting the long-running subsystems.
    if let Command::PayoutRunNow { dry_run } = command {
        return run_payout_now(&cfg, dry_run).await;
    }

    info!(
        instance = %cfg.instance_id,
        kaspad = %cfg.kaspad_url,
        stratum_port = %cfg.stratum_port,
        network = %cfg.network,
        pool_addresses = ?cfg.pool_addresses.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "katpool runtime starting"
    );

    // ---- DB pool -----------------------------------------------------
    let db = build_pool(&PoolConfig {
        url: cfg.database_url.clone(),
        min_connections: 2,
        max_connections: 16,
        application_name: format!("katpool[{}]", cfg.instance_id),
        ..PoolConfig::production("placeholder".to_owned())
    })
    .await
    .context("opening Postgres pool")?;

    // ---- shared event bus -------------------------------------------
    // Capacity sized for ~3 minutes of sustained 20 shares/s
    // (default 4096); operator-tunable for higher-throughput runs.
    let (event_tx, _event_rx_template) = broadcast::channel::<PoolEvent>(cfg.broadcast_capacity);

    if let Some(record_path) = &cfg.event_record_path {
        info!(path = %record_path, "PoolEvent NDJSON recorder enabled");
        spawn_event_recorder(event_tx.subscribe(), record_path.clone());
    }

    // ---- kaspad clients (bridge + accountant share the URL,
    //      separate connections) ---------------------------------------
    // Custodial PROP-pool mode: every block template the bridge
    // requests from kaspad pays the pool's address (regardless of
    // which miner authorized). The miner-supplied wallet on
    // `mining.authorize` becomes purely the share-credit identity;
    // the accountant pro-rates the matured coinbase across miners
    // by share weight; the payout engine (Phase 4) sends KAS to
    // each miner's authorized address.
    let coinbase_override = cfg
        .pool_addresses
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("KATPOOL_POOL_ADDRESS is empty"))?;
    if cfg.pool_addresses.len() > 1 {
        warn!(
            "multiple pool addresses supplied; bridge coinbase override uses the first ({coinbase_override}); accountant reward extraction matches against all"
        );
    }
    let kaspa_api = KaspaApi::new(
        cfg.kaspad_url.clone(),
        Duration::from_millis(500),
        None,
        Some(coinbase_override.clone()),
    )
    .await
    .map_err(|e| anyhow::anyhow!("KaspaApi: {e}"))?;
    let tracker_grpc = GrpcClient::connect_with_args(
        NotificationMode::Direct,
        cfg.kaspad_url.clone(),
        None,
        true,
        None,
        false,
        Some(500_000),
        Arc::default(),
    )
    .await
    .context("tracker gRPC connect")?;
    let tracker_kaspad = Arc::new(KaspadGrpcClient::new(
        Arc::new(tracker_grpc),
        cfg.pool_addresses.clone(),
    ));

    // ---- accountant pipeline ----------------------------------------
    let fee =
        FeeConfig::new(cfg.fee_topline_bps).map_err(|e| anyhow::anyhow!("fee config: {e}"))?;
    let engine = Arc::new(AllocationEngine::new(
        db.clone(),
        fee,
        Arc::new(StaticTierClassifier::standard()),
        cfg.instance_id.clone(),
    ));
    let tracker = MaturityTracker::new(
        db.clone(),
        tracker_kaspad,
        Arc::clone(&engine),
        cfg.maturity,
        cfg.instance_id.clone(),
    );

    // ---- public read-only API (Phase 6, opt-in) --------------------
    // Env-gated like the prom exporter (ADR-0021 A1). When enabled it owns
    // the `/health` `/ready` `/started` probes and the `/api/v1` surface.
    // Readiness reuses work the runtime already does: DB reachability from a
    // periodic `SELECT 1`, and kaspad-sync mirrored from the maturity
    // tracker's existing poll via a `watch` channel — so the API opens no
    // second gRPC connection. The tracker keeps the observer end; we keep
    // `tracker` shadowed with it attached only when the API is on.
    let tracker = if let Some(api_addr) = cfg.api_bind {
        let readiness = ReadinessHandle::new();
        let (sync_tx, sync_rx) = watch::channel(false);
        api::spawn_db_readiness_probe(db.clone(), readiness.clone());
        spawn_readiness_bridge(sync_rx, readiness.clone());
        let state = AppState::new(db.clone(), readiness, cfg.api_config.clone());
        tokio::spawn(async move {
            if let Err(e) = api::serve_on(api_addr, state).await {
                error!(error = %e, "public API server exited with error");
            }
        });
        info!(addr = %api_addr, "public read-only API enabled");
        tracker.with_sync_observer(sync_tx)
    } else {
        info!("public read-only API disabled (set KATPOOL_API_PORT to enable)");
        tracker
    };

    let consumer = EventConsumer::new(
        db.clone(),
        ConsumerConfig::new(cfg.instance_id.clone(), cfg.network.clone())
            .context("building accountant ConsumerConfig")?,
    );

    // ---- shutdown channel ------------------------------------------
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let signal_task = {
        let tx = shutdown_tx.clone();
        tokio::spawn(async move {
            tokio::select! {
                res = signal::ctrl_c() => {
                    if res.is_ok() { info!("SIGINT received"); }
                }
                () = sigterm() => info!("SIGTERM received"),
            }
            if tx.send(true).is_err() {
                warn!("shutdown channel closed before signal arrived");
            }
        })
    };

    // ---- spawn the three subsystems ---------------------------------
    let event_rx = event_tx.subscribe();
    let consumer_handle = tokio::spawn({
        let consumer = consumer;
        async move { consumer.run(event_rx).await }
    });
    let tracker_handle = tokio::spawn({
        let rx = shutdown_rx.clone();
        async move { tracker.run_loop(rx).await }
    });

    // ---- KAS payout engine (M4.7, opt-in) ---------------------------
    // Single-leader periodic loop: a Postgres advisory lock elects one
    // instance per tick, so running multiple `katpool` replicas is safe.
    // Disabled and dry-run by default — moving funds requires both
    // `KATPOOL_PAYOUT_ENABLED=true` and `KATPOOL_PAYOUT_DRY_RUN=false`.
    let payout_handle = if cfg.payout_enabled {
        let secret = match &cfg.payout.key_source {
            KeySource::File(path) => load_from_path(path)
                .with_context(|| format!("loading treasury key from {}", path.display()))?,
            KeySource::SystemdCredential(name) => load_from_systemd_credential(name)
                .with_context(|| format!("loading treasury credential `{name}`"))?,
        };
        let payout_client = GrpcKaspadClient::connect(cfg.kaspad_url.clone())
            .await
            .context("payout-kas kaspad gRPC connect")?;
        let mode = if cfg.payout.dry_run {
            ExecutionMode::DryRun
        } else {
            ExecutionMode::Live
        };
        let engine = PayoutEngine::new(
            db.clone(),
            payout_client,
            secret,
            coinbase_override.clone(),
            PayoutEngineConfig {
                instance_id: cfg.instance_id.clone(),
                poll_interval: cfg.payout.poll_interval,
                cycle_span_daa: cfg.payout.cycle_span_daa,
                threshold_sompi: cfg.payout.threshold_sompi,
                mode,
                lock_namespace: "payout-kas:kas-leader".to_owned(),
            },
        )
        .context("building payout engine")?;
        info!(
            dry_run = cfg.payout.dry_run,
            poll_secs = cfg.payout.poll_interval.as_secs(),
            cycle_span_daa = cfg.payout.cycle_span_daa,
            treasury = %coinbase_override,
            "payout-kas engine enabled"
        );
        let rx = shutdown_rx.clone();
        Some(tokio::spawn(async move { engine.run_loop(rx).await }))
    } else {
        info!("payout-kas engine disabled (set KATPOOL_PAYOUT_ENABLED=true to enable)");
        None
    };

    // ---- KRC-20 NACHO payout engine (M5.5b, opt-in) -----------------
    // Same single-leader discipline as the KAS engine, but a distinct
    // advisory-lock namespace so the two never contend. Shares the treasury
    // key/address and kaspad node (separate gRPC connection). Disabled and
    // dry-run by default.
    let krc20_payout_handle = if cfg.krc20_payout_enabled {
        let secret = match &cfg.krc20_payout.key_source {
            KeySource::File(path) => load_from_path(path)
                .with_context(|| format!("loading treasury key from {}", path.display()))?,
            KeySource::SystemdCredential(name) => load_from_systemd_credential(name)
                .with_context(|| format!("loading treasury credential `{name}`"))?,
        };
        let krc20_client = GrpcKaspadClient::connect(cfg.kaspad_url.clone())
            .await
            .context("payout-krc20 kaspad gRPC connect")?;
        let mode = if cfg.krc20_payout.dry_run {
            ExecutionMode::DryRun
        } else {
            ExecutionMode::Live
        };
        let quote = BreakeredSource::new(
            KaspaComFloorPrice::new(cfg.krc20_payout.quote_base.clone(), DEFAULT_HTTP_TIMEOUT)
                .context("building NACHO floor-price client")?,
            CircuitBreaker::new(
                cfg.krc20_payout.breaker_threshold,
                cfg.krc20_payout.breaker_cooldown,
            ),
        );
        let engine = Krc20PayoutEngine::new(
            db.clone(),
            krc20_client,
            secret,
            coinbase_override.clone(),
            quote,
            Krc20PayoutEngineConfig {
                instance_id: cfg.instance_id.clone(),
                poll_interval: cfg.krc20_payout.poll_interval,
                cycle_span_daa: cfg.krc20_payout.cycle_span_daa,
                mode,
                lock_namespace: "payout-krc20:nacho-leader".to_owned(),
                min_pending_sompi: cfg.krc20_payout.min_pending_sompi,
                min_nacho_base_units: cfg.krc20_payout.min_nacho_base_units,
                ticker: cfg.krc20_payout.ticker.clone(),
                commit_amount_sompi: cfg.krc20_payout.commit_amount_sompi,
                batch_limit: cfg.krc20_payout.batch_limit,
            },
        )
        .context("building krc20 payout engine")?;
        info!(
            dry_run = cfg.krc20_payout.dry_run,
            poll_secs = cfg.krc20_payout.poll_interval.as_secs(),
            cycle_span_daa = cfg.krc20_payout.cycle_span_daa,
            ticker = %cfg.krc20_payout.ticker,
            treasury = %coinbase_override,
            "payout-krc20 engine enabled"
        );
        let rx = shutdown_rx.clone();
        Some(tokio::spawn(async move { engine.run_loop(rx).await }))
    } else {
        info!("payout-krc20 engine disabled (set KATPOOL_KRC20_PAYOUT_ENABLED=true to enable)");
        None
    };

    // Bridge is the long-running stratum listener. Its
    // `listen_and_serve_with_events` doesn't currently respect a
    // shutdown channel (upstream limitation); we drive its lifetime
    // off the JoinHandle here and depend on SIGTERM/SIGINT killing
    // the process when we want it down.
    let bridge_config = BridgeServerConfig {
        instance_id: cfg.instance_id.clone(),
        stratum_port: cfg.stratum_port.clone(),
        kaspad_address: cfg.kaspad_url.clone(),
        prom_port: cfg.prom_port.clone(),
        print_stats: false,
        log_to_file: false,
        health_check_port: cfg.health_check_port.clone(),
        block_wait_time: Duration::from_millis(500),
        min_share_diff: cfg.min_share_diff,
        var_diff: cfg.var_diff,
        shares_per_min: cfg.shares_per_min,
        var_diff_stats: false,
        extranonce_size: 2,
        pow2_clamp: true,
        coinbase_tag_suffix: None,
    };
    let bridge_tx = event_tx.clone();
    let bridge_api = Arc::clone(&kaspa_api);
    let bridge_concrete = Some(Arc::clone(&kaspa_api));
    let bridge_handle = tokio::spawn(async move {
        listen_and_serve_with_events(bridge_config, bridge_api, bridge_concrete, Some(bridge_tx))
            .await
    });

    // Export Prometheus metrics when KATPOOL_PROM_PORT is set. The unified
    // runtime must start this itself — unlike the standalone bridge binary,
    // `listen_and_serve_with_events` does not. `start_prom_server` also runs
    // `init_metrics()`; without it every bridge `record_*` call is a no-op, so
    // this is what activates the anti-abuse counters as well as the exporter.
    if cfg.prom_port.is_empty() {
        info!("prometheus metrics disabled (set KATPOOL_PROM_PORT to enable)");
    } else {
        let prom_port = cfg.prom_port.clone();
        let prom_instance = cfg.instance_id.clone();
        info!(port = %prom_port, "prometheus metrics server enabled");
        tokio::spawn(async move {
            if let Err(e) = prom::start_prom_server(&prom_port, &prom_instance).await {
                error!("prometheus metrics server error: {e}");
            }
        });
    }

    info!("subsystems running; awaiting shutdown signal");

    // ---- wait for shutdown ------------------------------------------
    let mut shutdown_observer = shutdown_rx;
    let _ = shutdown_observer.changed().await;
    info!("shutdown signal observed; tearing down subsystems");

    // Shutdown semantics by subsystem:
    //
    // - **Tracker** has a clean shutdown channel (`shutdown_rx`)
    //   and exits at its next interval tick after the signal.
    // - **Bridge** and **consumer** do not yet have clean
    //   shutdown semantics:
    //   * the bridge's `listen_and_serve_with_events` spawns
    //     internal kaspad-notification tasks (per the upstream
    //     impl) that hold cloned `Arc<ShareHandler>` past the
    //     listener-task abort, which keeps clones of the
    //     `broadcast::Sender<PoolEvent>` alive;
    //   * the consumer drains until every Sender is dropped.
    //   The combination means a "drain to RecvError::Closed"
    //   path blocks indefinitely.
    //
    // Pragmatic M3d shutdown: abort the bridge + consumer
    // JoinHandles after the tracker is done. At-most-once
    // delivery is the design (lossy at restart is the
    // documented contract), so dropping in-flight events on
    // shutdown is correct. Phase 7's wiring rework will replace
    // the abort path with a clean shutdown if upstream grows
    // one.
    drop(event_tx);
    if let Err(e) = tracker_handle.await? {
        error!(error = %e, "tracker exited with error");
    }
    // The payout engines honor the shutdown channel; await them cleanly.
    if let Some(handle) = payout_handle {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => error!(error = %e, "payout engine exited with error"),
            Err(e) => error!(error = %e, "payout engine task join error"),
        }
    }
    if let Some(handle) = krc20_payout_handle {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => error!(error = %e, "krc20 payout engine exited with error"),
            Err(e) => error!(error = %e, "krc20 payout engine task join error"),
        }
    }
    bridge_handle.abort();
    consumer_handle.abort();
    let _ = bridge_handle.await;
    let _ = consumer_handle.await;
    signal_task.abort();
    let _ = signal_task.await;

    info!("katpool runtime exiting cleanly");
    Ok(())
}

/// CLI command selected from process arguments.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    /// Run the full pool runtime (default; no arguments).
    Daemon,
    /// Trigger a single KAS payout cycle synchronously, then exit.
    PayoutRunNow {
        /// Force dry-run (sign + verify only) regardless of `KATPOOL_PAYOUT_DRY_RUN`.
        dry_run: bool,
    },
    /// Print usage and exit.
    Help,
}

/// Parse process arguments (excluding `argv[0]`) into a [`Command`].
///
/// Kept dependency-free and pure so it is exhaustively unit-testable. The
/// daemon is the default so the systemd unit (which passes no arguments)
/// is unaffected.
fn parse_args(args: &[String]) -> Result<Command> {
    let mut iter = args.iter().map(String::as_str);
    match iter.next() {
        None => Ok(Command::Daemon),
        Some("-h" | "--help" | "help") => Ok(Command::Help),
        Some("payout") => match iter.next() {
            Some("run-now") => {
                let mut dry_run = false;
                for arg in iter {
                    match arg {
                        "--dry-run" => dry_run = true,
                        other => anyhow::bail!("unknown flag for `payout run-now`: {other}"),
                    }
                }
                Ok(Command::PayoutRunNow { dry_run })
            }
            Some(other) => {
                anyhow::bail!("unknown `payout` subcommand: {other} (expected `run-now`)")
            }
            None => anyhow::bail!("`payout` requires a subcommand (e.g. `run-now`)"),
        },
        Some(other) => anyhow::bail!("unknown command: {other} (try `--help`)"),
    }
}

// Help text is operator-facing and must reach the terminal regardless of the
// tracing filter, so stdout is correct here (unlike runtime diagnostics).
#[allow(clippy::print_stdout)]
fn print_usage() {
    println!(
        "katpool — Kaspa mining pool runtime\n\n\
         USAGE:\n  \
         katpool                          Run the full pool daemon (default)\n  \
         katpool payout run-now [--dry-run]\n                                   \
         Drive one KAS payout cycle now, then exit\n  \
         katpool --help                   Show this help\n\n\
         Configuration is environment-variable driven (see the module docs and\n\
         ops/env/<network>.env). `payout run-now` honours the same settings as\n\
         the daemon — including KATPOOL_PAYOUT_DRY_RUN — and coordinates with a\n\
         running daemon through the shared payout leader lock, so only one cycle\n\
         driver acts at a time. Pass `--dry-run` to preview without broadcasting."
    );
}

/// Operator on-demand payout: drive the current DAA-window cycle exactly as a
/// single daemon tick would (plan → broadcast → confirm → reconcile), under the
/// shared `payout-kas:kas-leader` advisory lock, then exit.
///
/// Safe to invoke while the daemon runs: the advisory lock guarantees only one
/// cycle driver acts at a time. If the daemon is mid-tick the lock is briefly
/// retried before giving up.
async fn run_payout_now(cfg: &RuntimeConfig, force_dry_run: bool) -> Result<()> {
    let db = build_pool(&PoolConfig {
        url: cfg.database_url.clone(),
        min_connections: 1,
        max_connections: 4,
        application_name: format!("katpool-payout-run-now[{}]", cfg.instance_id),
        ..PoolConfig::production("placeholder".to_owned())
    })
    .await
    .context("opening Postgres pool")?;

    let treasury_address = cfg
        .pool_addresses
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("KATPOOL_POOL_ADDRESS is empty"))?;

    let secret = match &cfg.payout.key_source {
        KeySource::File(path) => load_from_path(path)
            .with_context(|| format!("loading treasury key from {}", path.display()))?,
        KeySource::SystemdCredential(name) => load_from_systemd_credential(name)
            .with_context(|| format!("loading treasury credential `{name}`"))?,
    };

    let client = GrpcKaspadClient::connect(cfg.kaspad_url.clone())
        .await
        .context("payout-kas kaspad gRPC connect")?;

    let mode = if force_dry_run || cfg.payout.dry_run {
        ExecutionMode::DryRun
    } else {
        ExecutionMode::Live
    };

    let engine = PayoutEngine::new(
        db,
        client,
        secret,
        treasury_address.clone(),
        PayoutEngineConfig {
            instance_id: cfg.instance_id.clone(),
            poll_interval: cfg.payout.poll_interval,
            cycle_span_daa: cfg.payout.cycle_span_daa,
            threshold_sompi: cfg.payout.threshold_sompi,
            mode,
            lock_namespace: "payout-kas:kas-leader".to_owned(),
        },
    )
    .context("building payout engine")?;

    info!(
        dry_run = mode.is_dry_run(),
        threshold_sompi = cfg.payout.threshold_sompi,
        cycle_span_daa = cfg.payout.cycle_span_daa,
        treasury = %treasury_address,
        "payout run-now: driving current cycle"
    );

    // The daemon may hold the leader lock mid-tick; retry briefly before failing.
    let mut attempt = 0_u32;
    let report = loop {
        attempt += 1;
        match engine.run_once().await.context("payout run-now tick")? {
            TickOutcome::Ran(report) => break report,
            TickOutcome::SkippedNotLeader if attempt < 10 => {
                warn!(attempt, "payout leader lock held elsewhere; retrying in 1s");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            TickOutcome::SkippedNotLeader => {
                anyhow::bail!("another instance holds the payout leader lock; try again shortly");
            }
        }
    };

    let broadcast = &report.broadcast;
    info!(
        cycle_id = report.cycle_id,
        status = ?report.status,
        dry_run = mode.is_dry_run(),
        planned_batches = broadcast.planned_batches,
        submitted_payouts = broadcast.submitted_payouts,
        accepted = report.confirm.accepted,
        confirmed = report.confirm.confirmed,
        deferred_below_floor = broadcast.deferred_below_floor,
        unpaid = broadcast.unpaid,
        "payout run-now complete"
    );
    if !broadcast.submit_errors.is_empty() {
        error!(
            errors = broadcast.submit_errors.len(),
            detail = %broadcast.submit_errors.join("; "),
            "payout run-now: broadcast(s) rejected"
        );
        anyhow::bail!(
            "{} payout broadcast(s) were rejected",
            broadcast.submit_errors.len()
        );
    }
    Ok(())
}

async fn sigterm() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut sig) = signal(SignalKind::terminate()) {
            sig.recv().await;
        }
    }
    #[cfg(not(unix))]
    {
        std::future::pending::<()>().await;
    }
}

#[derive(Debug)]
struct RuntimeConfig {
    kaspad_url: String,
    database_url: String,
    pool_addresses: Vec<Address>,
    stratum_port: String,
    prom_port: String,
    health_check_port: String,
    instance_id: String,
    fee_topline_bps: u16,
    /// Per-miner stratum difficulty floor and (when `var_diff` is off)
    /// pin point. ASIC-class default is 4096; vardiff lifts from here.
    min_share_diff: u32,
    /// Enable the bridge's variable-difficulty retarget loop. When `false`,
    /// every connection is pinned at [`Self::min_share_diff`] for its
    /// lifetime, which on a fast-block-rate network like Kaspa causes
    /// ASIC-class miners to flood low-difficulty shares that go stale
    /// against newer block templates.
    var_diff: bool,
    /// Target accepted-shares-per-minute that the vardiff retarget loop
    /// converges each miner toward; ignored when `var_diff` is `false`.
    shares_per_min: u32,
    broadcast_capacity: usize,
    maturity: MaturityConfig,
    /// Network identifier passed to the accountant for
    /// `wallet::ensure`. One of `mainnet`, `testnet-10`,
    /// `testnet-11`, `devnet`, `simnet` (see
    /// [`accountant::consumer::VALID_NETWORKS`]). Derived from the
    /// pool address prefix unless `KATPOOL_NETWORK` overrides it
    /// (testnet-11 must be set explicitly because the bech32 prefix
    /// is shared with testnet-10).
    network: String,
    /// When set, append one serde-json `PoolEvent` per line to this path.
    event_record_path: Option<String>,
    /// Bind address for the public read-only API (`KATPOOL_API_PORT`).
    /// `None` disables the API (mirrors the prom exporter's env gate).
    api_bind: Option<SocketAddr>,
    /// API knobs (rate limit, cache TTLs, timeout, CORS). Parsed
    /// unconditionally; only consumed when `api_bind` is `Some`.
    api_config: ApiConfig,
    /// Whether the KAS payout engine runs in this process.
    payout_enabled: bool,
    /// Payout engine knobs (parsed unconditionally; only consumed when
    /// `payout_enabled`).
    payout: PayoutConfig,
    /// Whether the KRC-20 NACHO payout engine runs in this process.
    krc20_payout_enabled: bool,
    /// KRC-20 engine knobs (parsed unconditionally; only consumed when
    /// `krc20_payout_enabled`).
    krc20_payout: Krc20RuntimeConfig,
}

/// Where the treasury signing key is loaded from at startup.
#[derive(Debug, Clone)]
enum KeySource {
    /// systemd `LoadCredentialEncrypted` credential name (production).
    SystemdCredential(String),
    /// Raw 32-byte hex key file (testnet rehearsal / M4.8).
    File(PathBuf),
}

/// Parsed KAS payout engine configuration.
#[derive(Debug)]
struct PayoutConfig {
    dry_run: bool,
    poll_interval: Duration,
    cycle_span_daa: u64,
    threshold_sompi: i64,
    key_source: KeySource,
}

/// Parsed KRC-20 NACHO payout engine configuration.
#[derive(Debug)]
struct Krc20RuntimeConfig {
    dry_run: bool,
    poll_interval: Duration,
    cycle_span_daa: u64,
    min_pending_sompi: i64,
    min_nacho_base_units: u128,
    commit_amount_sompi: u64,
    batch_limit: i64,
    ticker: String,
    quote_base: String,
    breaker_threshold: u32,
    breaker_cooldown: Duration,
    key_source: KeySource,
}

impl Krc20RuntimeConfig {
    fn from_env(key_source: KeySource) -> Result<Self> {
        Ok(Self {
            dry_run: optional_bool("KATPOOL_KRC20_PAYOUT_DRY_RUN")?.unwrap_or(true),
            poll_interval: Duration::from_secs(
                optional_u64("KATPOOL_KRC20_PAYOUT_POLL_SECS")?.unwrap_or(60),
            ),
            cycle_span_daa: optional_u64("KATPOOL_KRC20_PAYOUT_CYCLE_SPAN_DAA")?.unwrap_or(216_000),
            min_pending_sompi: optional_i64("KATPOOL_KRC20_MIN_PENDING_SOMPI")?
                .unwrap_or(DEFAULT_MIN_PENDING_SOMPI),
            min_nacho_base_units: optional_u128("KATPOOL_KRC20_MIN_NACHO_BASE_UNITS")?
                .unwrap_or(DEFAULT_MIN_NACHO_BASE_UNITS),
            commit_amount_sompi: optional_u64("KATPOOL_KRC20_COMMIT_AMOUNT_SOMPI")?
                .unwrap_or(DEFAULT_COMMIT_AMOUNT_SOMPI),
            batch_limit: optional_i64("KATPOOL_KRC20_BATCH_LIMIT")?.unwrap_or(DEFAULT_CYCLE_LIMIT),
            ticker: optional("KATPOOL_KRC20_TICKER")
                .unwrap_or_else(|| DEFAULT_QUOTE_TICKER.to_owned()),
            quote_base: optional("KATPOOL_KRC20_QUOTE_BASE")
                .unwrap_or_else(|| DEFAULT_QUOTE_BASE.to_owned()),
            breaker_threshold: optional_u32("KATPOOL_KRC20_QUOTE_BREAKER_THRESHOLD")?.unwrap_or(3),
            breaker_cooldown: Duration::from_secs(
                optional_u64("KATPOOL_KRC20_QUOTE_BREAKER_COOLDOWN_SECS")?.unwrap_or(60),
            ),
            key_source,
        })
    }
}

impl RuntimeConfig {
    fn from_env() -> Result<Self> {
        let kaspad_url = required("KASPAD_GRPC_URL")?;
        let database_url = required("KATPOOL_DATABASE_URL")?;
        let stratum_port = required("KATPOOL_STRATUM_PORT")?;
        let pool_address_raw = required("KATPOOL_POOL_ADDRESS")?;
        let pool_addresses = pool_address_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                Address::try_from(s)
                    .map_err(|e| anyhow::anyhow!("KATPOOL_POOL_ADDRESS entry `{s}`: {e}"))
            })
            .collect::<Result<Vec<_>>>()?;
        if pool_addresses.is_empty() {
            anyhow::bail!("KATPOOL_POOL_ADDRESS produced an empty list");
        }
        let instance_id =
            optional("KATPOOL_INSTANCE_ID").unwrap_or_else(|| "katpool-runtime".to_owned());
        let prom_port = optional("KATPOOL_PROM_PORT").unwrap_or_default();
        let health_check_port = optional("KATPOOL_HEALTH_CHECK_PORT").unwrap_or_default();
        let fee_topline_bps = optional_u16("KATPOOL_FEE_TOPLINE_BPS")?.unwrap_or(75);
        let min_share_diff = optional_u32("KATPOOL_MIN_SHARE_DIFF")?.unwrap_or(4096);
        let var_diff = optional_bool("KATPOOL_VAR_DIFF")?.unwrap_or(true);
        let shares_per_min = optional_u32("KATPOOL_SHARES_PER_MIN")?.unwrap_or(20);
        let broadcast_capacity = optional_usize("KATPOOL_BROADCAST_CAPACITY")?.unwrap_or(4096);
        let poll_secs = optional_u64("KATPOOL_MATURITY_POLL_SECS")?.unwrap_or(15);
        let coinbase_maturity = optional_u64("KATPOOL_COINBASE_MATURITY")?.unwrap_or(1000);
        let window_daa_span = optional_u64("KATPOOL_WINDOW_DAA_SPAN")?.unwrap_or(600);
        let batch_size = optional_i64("KATPOOL_MATURITY_BATCH_SIZE")?.unwrap_or(200);
        let network = resolve_network(&pool_addresses)?;
        let event_record_path = optional("KATPOOL_EVENT_RECORD_PATH");

        // Public read-only API (Phase 6). `KATPOOL_API_PORT` is a full bind
        // address (`host:port`), mirroring `KATPOOL_PROM_PORT`; empty disables.
        let api_bind = optional("KATPOOL_API_PORT")
            .map(|s| {
                s.parse::<SocketAddr>().map_err(|e| {
                    anyhow::anyhow!(
                        "KATPOOL_API_PORT=`{s}`: {e} (expected host:port, e.g. 127.0.0.1:8080)"
                    )
                })
            })
            .transpose()?;
        let api_config =
            ApiConfig::from_env().map_err(|e| anyhow::anyhow!("API configuration: {e}"))?;

        let payout_enabled = optional_bool("KATPOOL_PAYOUT_ENABLED")?.unwrap_or(false);
        let payout_dry_run = optional_bool("KATPOOL_PAYOUT_DRY_RUN")?.unwrap_or(true);
        let payout_poll_secs = optional_u64("KATPOOL_PAYOUT_POLL_SECS")?.unwrap_or(60);
        let payout_cycle_span_daa =
            optional_u64("KATPOOL_PAYOUT_CYCLE_SPAN_DAA")?.unwrap_or(216_000);
        let payout_threshold_sompi = optional_i64("KATPOOL_PAYOUT_THRESHOLD_SOMPI")?
            .unwrap_or(DEFAULT_KAS_PAYOUT_THRESHOLD_SOMPI);
        let key_source = optional("KATPOOL_TREASURY_KEY_PATH").map_or_else(
            || {
                KeySource::SystemdCredential(
                    optional("KATPOOL_TREASURY_CREDENTIAL")
                        .unwrap_or_else(|| "treasury-key".to_owned()),
                )
            },
            |path| KeySource::File(PathBuf::from(path)),
        );
        let payout = PayoutConfig {
            dry_run: payout_dry_run,
            poll_interval: Duration::from_secs(payout_poll_secs),
            cycle_span_daa: payout_cycle_span_daa,
            threshold_sompi: payout_threshold_sompi,
            key_source: key_source.clone(),
        };

        // KRC-20 NACHO payout engine (shares the treasury key source).
        let krc20_payout_enabled = optional_bool("KATPOOL_KRC20_PAYOUT_ENABLED")?.unwrap_or(false);
        let krc20_payout = Krc20RuntimeConfig::from_env(key_source)?;

        Ok(Self {
            kaspad_url,
            database_url,
            pool_addresses,
            stratum_port,
            prom_port,
            health_check_port,
            instance_id,
            fee_topline_bps,
            min_share_diff,
            var_diff,
            shares_per_min,
            broadcast_capacity,
            network,
            event_record_path,
            api_bind,
            api_config,
            maturity: MaturityConfig {
                poll_interval: Duration::from_secs(poll_secs),
                coinbase_maturity,
                window_daa_span,
                batch_size,
            },
            payout_enabled,
            payout,
            krc20_payout_enabled,
            krc20_payout,
        })
    }
}

fn required(var: &str) -> Result<String> {
    std::env::var(var).map_err(|_| anyhow::anyhow!("required env var {var} unset"))
}

fn optional(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|s| !s.is_empty())
}

fn optional_u16(var: &str) -> Result<Option<u16>> {
    optional(var)
        .map(|s| {
            s.parse::<u16>()
                .map_err(|e| anyhow::anyhow!("{var}=`{s}`: {e}"))
        })
        .transpose()
}

fn optional_u32(var: &str) -> Result<Option<u32>> {
    optional(var)
        .map(|s| {
            s.parse::<u32>()
                .map_err(|e| anyhow::anyhow!("{var}=`{s}`: {e}"))
        })
        .transpose()
}

fn optional_u64(var: &str) -> Result<Option<u64>> {
    optional(var)
        .map(|s| {
            s.parse::<u64>()
                .map_err(|e| anyhow::anyhow!("{var}=`{s}`: {e}"))
        })
        .transpose()
}

fn optional_i64(var: &str) -> Result<Option<i64>> {
    optional(var)
        .map(|s| {
            s.parse::<i64>()
                .map_err(|e| anyhow::anyhow!("{var}=`{s}`: {e}"))
        })
        .transpose()
}

fn optional_u128(var: &str) -> Result<Option<u128>> {
    optional(var)
        .map(|s| {
            s.parse::<u128>()
                .map_err(|e| anyhow::anyhow!("{var}=`{s}`: {e}"))
        })
        .transpose()
}

fn optional_bool(var: &str) -> Result<Option<bool>> {
    optional(var)
        .map(|s| match s.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" => Ok(false),
            other => Err(anyhow::anyhow!(
                "{var}=`{other}`: expected a boolean (true/false/1/0/yes/no/on/off)"
            )),
        })
        .transpose()
}

fn optional_usize(var: &str) -> Result<Option<usize>> {
    optional(var)
        .map(|s| {
            s.parse::<usize>()
                .map_err(|e| anyhow::anyhow!("{var}=`{s}`: {e}"))
        })
        .transpose()
}

/// Resolve the schema-network identifier for `wallet::ensure`.
///
/// Order of precedence:
/// 1. `KATPOOL_NETWORK` env override (required for `testnet-11`,
///    `devnet`, `simnet` because their bech32 prefixes overlap
///    other targets).
/// 2. Derived from the first pool address bech32 prefix — `kaspa:` →
///    `mainnet`, `kaspatest:` → `testnet-10` (the active testnet at
///    the time of writing; override via `KATPOOL_NETWORK` for
///    testnet-11).
///
/// The returned string is validated against
/// [`accountant::consumer::VALID_NETWORKS`] (matching the DB CHECK
/// constraint) so a misconfiguration fails fast on startup instead
/// of being discovered at the first `wallet::ensure` call.
/// Mirror the maturity tracker's kaspad-reachability signal into the API
/// [`ReadinessHandle`].
///
/// Each sweep publishes `true`/`false` on `sync_rx`; this task forwards that to
/// `kaspad_synced` and latches `started` the first time reachability is
/// observed. It exits when the tracker (the sender) is gone. This is the
/// "reuse existing kaspad polling, no second connection" wiring from ADR-0021.
fn spawn_readiness_bridge(mut sync_rx: watch::Receiver<bool>, readiness: ReadinessHandle) {
    tokio::spawn(async move {
        loop {
            let synced = *sync_rx.borrow_and_update();
            readiness.set_kaspad_synced(synced);
            if synced {
                readiness.mark_started();
            }
            if sync_rx.changed().await.is_err() {
                break;
            }
        }
    });
}

/// Append-only NDJSON capture of every `PoolEvent` on the bus.
fn spawn_event_recorder(mut rx: broadcast::Receiver<PoolEvent>, path: String) {
    tokio::spawn(async move {
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                error!(path = %path, error = %e, "event recorder: cannot open output file");
                return;
            }
        };
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let line = match serde_json::to_string(&event) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(error = %e, "event recorder: serialize failed");
                            continue;
                        }
                    };
                    if file.write_all(line.as_bytes()).await.is_err()
                        || file.write_all(b"\n").await.is_err()
                    {
                        error!(path = %path, "event recorder: write failed");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        skipped,
                        "event recorder lagged behind broadcast; events dropped"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn resolve_network(pool_addresses: &[Address]) -> Result<String> {
    let resolved = if let Some(override_value) = optional("KATPOOL_NETWORK") {
        override_value
    } else {
        let first = pool_addresses
            .first()
            .ok_or_else(|| anyhow::anyhow!("resolve_network: pool_addresses empty"))?;
        match first.prefix {
            Prefix::Mainnet => "mainnet".to_owned(),
            Prefix::Testnet => "testnet-10".to_owned(),
            Prefix::Devnet => "devnet".to_owned(),
            Prefix::Simnet => "simnet".to_owned(),
        }
    };
    if !accountant::VALID_NETWORKS.contains(&resolved.as_str()) {
        anyhow::bail!(
            "KATPOOL_NETWORK=`{resolved}` not in {:?}",
            accountant::VALID_NETWORKS
        );
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_args};

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn no_args_runs_daemon() {
        assert_eq!(parse_args(&args(&[])).ok(), Some(Command::Daemon));
    }

    #[test]
    fn help_flags_request_usage() {
        for flag in ["-h", "--help", "help"] {
            assert_eq!(parse_args(&args(&[flag])).ok(), Some(Command::Help));
        }
    }

    #[test]
    fn payout_run_now_defaults_to_live() {
        assert_eq!(
            parse_args(&args(&["payout", "run-now"])).ok(),
            Some(Command::PayoutRunNow { dry_run: false })
        );
    }

    #[test]
    fn payout_run_now_accepts_dry_run_flag() {
        assert_eq!(
            parse_args(&args(&["payout", "run-now", "--dry-run"])).ok(),
            Some(Command::PayoutRunNow { dry_run: true })
        );
    }

    #[test]
    fn unknown_payout_subcommand_errors() {
        assert!(parse_args(&args(&["payout", "bogus"])).is_err());
    }

    #[test]
    fn payout_without_subcommand_errors() {
        assert!(parse_args(&args(&["payout"])).is_err());
    }

    #[test]
    fn unknown_flag_for_run_now_errors() {
        assert!(parse_args(&args(&["payout", "run-now", "--wat"])).is_err());
    }

    #[test]
    fn unknown_top_level_command_errors() {
        assert!(parse_args(&args(&["frobnicate"])).is_err());
    }
}
