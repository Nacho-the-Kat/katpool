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
//!    separate client) for blue-score / block-info; transitions
//!    blocks `submitted_to_node → confirmed_blue → matured` and
//!    calls the accountant's allocation engine on maturity.
//!
//! All three subsystems shut down cleanly on SIGINT / SIGTERM via
//! a `tokio::sync::watch::Receiver<bool>` propagated from the
//! signal task.
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
//! - `KATPOOL_MIN_SHARE_DIFF`        default 1
//! - `KATPOOL_PROM_PORT`             default empty (disabled)
//! - `KATPOOL_HEALTH_CHECK_PORT`     default empty (disabled)
//! - `KATPOOL_MATURITY_POLL_SECS`    default 15
//! - `KATPOOL_MATURITY_DEPTH`        default 100
//! - `KATPOOL_WINDOW_DAA_SPAN`       default 600
//! - `KATPOOL_BROADCAST_CAPACITY`    default 4096
//! - `KATPOOL_EVENT_RECORD_PATH`     optional NDJSON `PoolEvent` capture
//!   for M4 replay-determinism rehearsal

#![cfg_attr(not(test), warn(missing_docs))]

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use kaspa_addresses::{Address, Prefix};
use kaspa_grpc_client::GrpcClient;
use kaspa_rpc_core::notify::mode::NotificationMode;
use kaspa_stratum_bridge::{
    BridgeConfig as BridgeServerConfig, KaspaApi, listen_and_serve_with_events,
};
use katpool_db::{PoolConfig, build_pool};
use katpool_domain::PoolEvent;
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

    let cfg = RuntimeConfig::from_env().context("loading runtime config")?;
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
        Some(coinbase_override),
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
        var_diff: false,
        shares_per_min: 0,
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
    bridge_handle.abort();
    consumer_handle.abort();
    let _ = bridge_handle.await;
    let _ = consumer_handle.await;
    signal_task.abort();
    let _ = signal_task.await;

    info!("katpool runtime exiting cleanly");
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
    min_share_diff: u32,
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
        let min_share_diff = optional_u32("KATPOOL_MIN_SHARE_DIFF")?.unwrap_or(1);
        let broadcast_capacity = optional_usize("KATPOOL_BROADCAST_CAPACITY")?.unwrap_or(4096);
        let poll_secs = optional_u64("KATPOOL_MATURITY_POLL_SECS")?.unwrap_or(15);
        let maturity_depth = optional_u64("KATPOOL_MATURITY_DEPTH")?.unwrap_or(100);
        let window_daa_span = optional_u64("KATPOOL_WINDOW_DAA_SPAN")?.unwrap_or(600);
        let batch_size = optional_i64("KATPOOL_MATURITY_BATCH_SIZE")?.unwrap_or(200);
        let network = resolve_network(&pool_addresses)?;
        let event_record_path = optional("KATPOOL_EVENT_RECORD_PATH");
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
            broadcast_capacity,
            network,
            event_record_path,
            maturity: MaturityConfig {
                poll_interval: Duration::from_secs(poll_secs),
                maturity_depth,
                window_daa_span,
                batch_size,
            },
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
