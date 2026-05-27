use crate::{
    client_handler::ClientHandler,
    default_client::{default_handlers, handle_authorize, handle_subscribe},
    jsonrpc_event::JsonRpcEvent,
    kaspaapi::KaspaApi,
    share_handler::{KaspaApiTrait, ShareHandler},
    stratum_context::StratumContext,
    stratum_listener::{StratumListener, StratumListenerConfig},
};
use katpool_domain::PoolEvent;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

pub struct BridgeConfig {
    pub instance_id: String, // Instance identifier for logging (e.g., "Instance 1", "Instance 2")
    pub stratum_port: String,
    pub kaspad_address: String,
    pub prom_port: String,
    pub print_stats: bool,
    pub log_to_file: bool,
    pub health_check_port: String,
    pub block_wait_time: Duration,
    pub min_share_diff: u32,
    pub var_diff: bool,
    pub shares_per_min: u32,
    pub var_diff_stats: bool,
    pub extranonce_size: u8,
    pub pow2_clamp: bool,
    pub coinbase_tag_suffix: Option<String>,
}

/// Start block template listener with concrete KaspaApi
/// This should be called from main.rs where we have concrete type
pub async fn start_block_template_listener_with_api(
    kaspa_api: Arc<KaspaApi>,
    block_wait_time: Duration,
    client_handler: Arc<ClientHandler>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client_handler_cb = Arc::clone(&client_handler);
    let kaspa_api_cb = Arc::clone(&kaspa_api);

    let block_cb = move || {
        let client_handler = Arc::clone(&client_handler_cb);
        let kaspa_api = Arc::clone(&kaspa_api_cb);
        tokio::spawn(async move {
            client_handler.new_block_available(kaspa_api).await;
        });
    };

    kaspa_api
        .start_block_template_listener(block_wait_time, block_cb)
        .await
        .map_err(|e| Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
}

pub async fn listen_and_serve<T: KaspaApiTrait + Send + Sync + 'static>(
    config: BridgeConfig,
    kaspa_api: Arc<T>,
    // Optional: if concrete KaspaApi is provided, use notification-based listener
    concrete_kaspa_api: Option<Arc<KaspaApi>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    listen_and_serve_with_events(config, kaspa_api, concrete_kaspa_api, None).await
}

/// `listen_and_serve` plus an optional broadcast sender for
/// `PoolEvent`s.
///
/// katpool fork addition. When `event_tx` is provided, the
/// internal `ShareHandler` is wired via
/// [`ShareHandler::with_event_bus`] and every share / block
/// lifecycle event the handler emits goes into the channel.
/// This is the seam the unified `katpool` runtime binary uses to
/// connect the bridge to the accountant in the same process.
///
/// Pass `None` to get identical behaviour to the original
/// `listen_and_serve` (the upstream call shape).
///
/// Logged divergence per `bridge/UPSTREAM.md`.
pub async fn listen_and_serve_with_events<T: KaspaApiTrait + Send + Sync + 'static>(
    config: BridgeConfig,
    kaspa_api: Arc<T>,
    concrete_kaspa_api: Option<Arc<KaspaApi>>,
    event_tx: Option<broadcast::Sender<PoolEvent>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Calculate min diff with pow2 clamp if needed
    let mut min_diff = config.min_share_diff as f64;
    if config.pow2_clamp && min_diff > 0.0 {
        min_diff = 2_f64.powi((min_diff.log2().floor()) as i32);
    }
    if min_diff == 0.0 {
        min_diff = 4.0;
    }

    // Extranonce size is now auto-detected per client based on miner type
    // We still need to pass a value to ClientHandler::new() for backward compatibility,
    // but it will be ignored as extranonce is assigned per-client in handle_subscribe
    // Default to 2 (for IceRiver/BzMiner/Goldshell) as that's the most common case
    let extranonce_size = if config.extranonce_size > 0 {
        config.extranonce_size.min(3) as i8
    } else {
        2 // Default to 2, will be auto-detected per client anyway
    };

    // Create share handler with instance identifier. When the
    // caller supplied an event bus sender, wire it in so every
    // share + block lifecycle event flows to the downstream
    // accountant consumer.
    let instance_id = config.instance_id.clone();
    let share_handler = {
        let mut handler = ShareHandler::new(instance_id.clone());
        if let Some(tx) = event_tx {
            handler = handler.with_event_bus(tx);
        }
        Arc::new(handler)
    };

    // Create client handler
    // Note: extranonce_size parameter is now only used for backward compatibility
    // Actual extranonce assignment happens per-client in handle_subscribe based on detected miner type
    let client_handler = Arc::new(ClientHandler::new(Arc::clone(&share_handler), min_diff, extranonce_size, instance_id.clone()));

    // Setup default handlers
    let mut handlers = default_handlers();

    // Override subscribe handler to enable automatic extranonce detection
    let subscribe_handler = {
        let client_handler = Arc::clone(&client_handler);
        Arc::new(move |ctx: Arc<StratumContext>, event: JsonRpcEvent| {
            let client_handler = Arc::clone(&client_handler);
            let ctx_clone = Arc::clone(&ctx);
            let event_clone = event.clone();
            Box::pin(async move {
                handle_subscribe(ctx_clone, event_clone, Some(client_handler))
                    .await
                    .map_err(|e| Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send>>
        }) as crate::stratum_listener::EventHandler
    };
    handlers.insert("mining.subscribe".to_string(), subscribe_handler);

    // Override authorize handler to send immediate job (critical for IceRiver KS2L)
    let authorize_handler = {
        let client_handler = Arc::clone(&client_handler);
        let kaspa_api = Arc::clone(&kaspa_api);
        Arc::new(move |ctx: Arc<StratumContext>, event: JsonRpcEvent| {
            let client_handler = Arc::clone(&client_handler);
            let kaspa_api = Arc::clone(&kaspa_api);
            let ctx_clone = Arc::clone(&ctx);
            let event_clone = event.clone();
            Box::pin(async move {
                handle_authorize(ctx_clone, event_clone, Some(client_handler), Some(kaspa_api))
                    .await
                    .map_err(|e| Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send>>
        }) as crate::stratum_listener::EventHandler
    };
    handlers.insert("mining.authorize".to_string(), authorize_handler);

    // Override submit handler
    let submit_handler = {
        let share_handler = Arc::clone(&share_handler);
        let kaspa_api = Arc::clone(&kaspa_api);
        Arc::new(move |ctx: Arc<StratumContext>, event: JsonRpcEvent| {
            let share_handler = Arc::clone(&share_handler);
            let kaspa_api = Arc::clone(&kaspa_api);
            let ctx_clone = Arc::clone(&ctx);
            Box::pin(async move {
                share_handler
                    .handle_submit(ctx_clone, event, kaspa_api)
                    .await
                    .map_err(|e| Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
            })
                as std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send>>
        }) as crate::stratum_listener::EventHandler
    };
    handlers.insert("mining.submit".to_string(), submit_handler);

    // Setup listener config
    // Each client will get its own MiningState (created in stratum_listener)
    // Each client gets its own isolated state
    // Per-IP anti-abuse guard. Defaults are production-grade (256 conns
    // per IP, 100 frames/sec sustained, 200 burst). Operators tune
    // these via `AntiAbuseConfig` injected at start-up; the Phase 1
    // close-out milestone surfaces them through a CLI/env layer.
    // Per-IP anti-abuse guard. Defaults are production-grade (256
    // conns per IP, 100 frames/sec sustained, 200 burst). Operators
    // override individual limits via the `KATPOOL_ANTI_ABUSE_*`
    // environment variables (see `AntiAbuseConfig::from_lookup` docs
    // and `ops/systemd/katpool-bridge.conf.d/anti-abuse.conf.example`).
    // Malformed env values fail-fast at start-up rather than silently
    // falling back to defaults, so an operator typo never ships into
    // production unnoticed.
    let anti_abuse_config = crate::anti_abuse::AntiAbuseConfig::from_env()
        .map_err(|e| Box::new(std::io::Error::other(format!("anti-abuse config: {e}"))) as Box<dyn std::error::Error + Send + Sync>)?;
    tracing::info!(
        "[{}] anti-abuse: max_conn_per_ip={}, max_tracked_ips={}, frame_rate_per_sec={}, frame_burst={}",
        config.instance_id,
        anti_abuse_config.max_conn_per_ip,
        anti_abuse_config.max_tracked_ips,
        anti_abuse_config.frame_rate_per_sec,
        anti_abuse_config.frame_burst
    );
    let anti_abuse = std::sync::Arc::new(crate::anti_abuse::AntiAbuseGuard::new(anti_abuse_config));

    let listener_config = StratumListenerConfig {
        port: config.stratum_port.clone(),
        handler_map: Arc::new(handlers),
        on_connect: Arc::new({
            let client_handler = Arc::clone(&client_handler);
            move |ctx: Arc<StratumContext>| {
                client_handler.on_connect(ctx);
            }
        }),
        on_disconnect: Arc::new({
            let client_handler = Arc::clone(&client_handler);
            move |ctx: Arc<StratumContext>| {
                client_handler.on_disconnect(&ctx);
            }
        }),
        anti_abuse,
        instance_id: config.instance_id.clone(),
    };

    // Start vardiff thread if enabled
    if config.var_diff {
        let shares_per_min = if config.shares_per_min > 0 { config.shares_per_min } else { 20 };
        share_handler.start_vardiff_thread(shares_per_min, config.var_diff_stats, config.pow2_clamp);
    }

    // Start stats printing thread if enabled
    if config.print_stats {
        let shares_per_min = if config.shares_per_min > 0 { config.shares_per_min } else { 20 };
        share_handler.start_print_stats_thread(shares_per_min);
    }

    // Start stats pruning thread
    share_handler.start_prune_stats_thread();

    // Start block template listener with notifications + ticker fallback
    // This provides immediate notifications when new blocks are available, with polling as fallback

    // If concrete KaspaApi is provided, use notification-based listener
    // Otherwise, use polling only (fallback for trait objects)
    if let Some(concrete_api) = concrete_kaspa_api {
        // We have concrete KaspaApi - use notification-based listener
        let client_handler_cb = Arc::clone(&client_handler);
        let kaspa_api_cb = Arc::clone(&kaspa_api);

        let block_cb = move || {
            let client_handler = Arc::clone(&client_handler_cb);
            let kaspa_api = Arc::clone(&kaspa_api_cb);
            tokio::spawn(async move {
                client_handler.new_block_available(kaspa_api).await;
            });
        };

        // Start notification-based listener with ticker fallback
        // Method signature: start_block_template_listener(self: Arc<Self>, ...)
        // Call the method directly on Arc<KaspaApi> (it's an instance method taking Arc<Self>)
        if let Err(e) = concrete_api.start_block_template_listener(config.block_wait_time, block_cb).await {
            warn!("Failed to start notification-based block template listener: {}, falling back to polling", e);
            // Fall through to polling approach
        } else {
            // Successfully started notification-based listener
            debug!("Started notification-based block template listener");
        }
    } else {
        // No concrete KaspaApi provided - use polling only
        warn!("Using polling-based block template listener (concrete KaspaApi not provided, notifications not available)");

        let client_handler_poll = Arc::clone(&client_handler);
        let kaspa_api_poll = Arc::clone(&kaspa_api);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.block_wait_time);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                // Poll for new blocks
                client_handler_poll.new_block_available(Arc::clone(&kaspa_api_poll)).await;
            }
        });
    }

    // Start listener
    let listener = StratumListener::new(listener_config);
    info!("{} Starting stratum listener on {}", instance_id, config.stratum_port);
    listener.listen().await
}
