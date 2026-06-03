use crate::anti_abuse::AntiAbuseGuard;
use crate::jsonrpc_event::JsonRpcEvent;
use crate::log_colors::LogColors;
use crate::prom::{record_anti_abuse_connection_reject, record_anti_abuse_frame_limited, record_malformed_frame};
use crate::stratum_context::StratumContext;
use hex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Event handler function type
pub type EventHandler = Arc<
    dyn Fn(
            Arc<StratumContext>,
            JsonRpcEvent,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send>>
        + Send
        + Sync,
>;

/// Client listener trait
pub trait StratumClientListener: Send + Sync {
    fn on_connect(&self, ctx: Arc<StratumContext>);
    fn on_disconnect(&self, ctx: Arc<StratumContext>);
}

/// State generator function type
pub type StateGenerator = Box<dyn Fn() -> Arc<dyn std::any::Any + Send + Sync> + Send + Sync>;

/// Stratum listener statistics
#[derive(Debug, Default)]
pub struct StratumStats {
    pub disconnects: u64,
}

/// Configuration for the Stratum listener
pub struct StratumListenerConfig {
    pub handler_map: Arc<HashMap<String, EventHandler>>,
    pub on_connect: Arc<dyn Fn(Arc<StratumContext>) + Send + Sync>,
    pub on_disconnect: Arc<dyn Fn(Arc<StratumContext>) + Send + Sync>,
    pub port: String,
    /// Per-IP anti-abuse guard. Used for connection-cap and frame-rate
    /// checks on inbound traffic; pass [`AntiAbuseGuard::new`] with
    /// [`crate::anti_abuse::AntiAbuseConfig::unlimited`] to disable.
    pub anti_abuse: Arc<AntiAbuseGuard>,
    /// Identifier used for Prometheus metric labels emitted by the
    /// anti-abuse layer.
    pub instance_id: String,
    /// When `true`, every accepted connection MUST begin with a PROXY
    /// protocol v2 header (ADR-0022); the real client IP/port is parsed
    /// from it and used for all downstream logic (anti-abuse,
    /// attribution). Connections without a valid header are dropped.
    /// Enable only behind a trusted forwarder (the fly.io edge), with
    /// the origin's stratum ports firewalled to the forwarder egress.
    /// Default `false` => raw TCP peer address is used (unchanged).
    pub proxy_protocol: bool,
}

/// Stratum TCP listener
pub struct StratumListener {
    config: StratumListenerConfig,
    stats: Arc<parking_lot::Mutex<StratumStats>>,
    shutting_down: Arc<std::sync::atomic::AtomicBool>,
}

impl StratumListener {
    /// Create a new Stratum listener
    pub fn new(config: StratumListenerConfig) -> Self {
        Self {
            config,
            stats: Arc::new(parking_lot::Mutex::new(StratumStats::default())),
            shutting_down: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start listening for connections
    pub async fn listen(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.shutting_down.store(false, std::sync::atomic::Ordering::Release);

        // Parse port - ensure we bind to IPv4 (0.0.0.0) to accept IPv4 connections
        // If it starts with ':', prepend "0.0.0.0", otherwise format as "0.0.0.0:PORT"
        let addr_str = if self.config.port.starts_with(':') {
            format!("0.0.0.0{}", self.config.port)
        } else if self.config.port.chars().all(|c| c.is_ascii_digit()) {
            format!("0.0.0.0:{}", self.config.port)
        } else {
            self.config.port.clone()
        };

        let listener =
            TcpListener::bind(&addr_str).await.map_err(|e| format!("failed listening to socket {}: {}", self.config.port, e))?;

        debug!("Stratum listener started on {}", self.config.port);

        let (disconnect_tx, mut disconnect_rx) = mpsc::unbounded_channel::<Arc<StratumContext>>();
        let disconnect_tx_clone = disconnect_tx.clone();
        let on_disconnect = Arc::clone(&self.config.on_disconnect);
        let stats = self.stats.clone();

        // Spawn disconnect handler
        tokio::spawn(async move {
            while let Some(ctx) = disconnect_rx.recv().await {
                info!("[CONNECTION] client disconnecting - {}", ctx.remote_addr);
                info!("[CONNECTION] Disconnect event for {}:{}", ctx.remote_addr, ctx.remote_port);
                stats.lock().disconnects += 1;
                on_disconnect(ctx);
            }
        });

        // Accept connections
        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((mut stream, addr)) => {
                            // Local (listening) port this connection landed on.
                            // Drives the per-port starting-difficulty seed
                            // (ADR-0022). `local_addr()` is authoritative even
                            // with SO_REUSEPORT / multiple listeners.
                            let local_port = stream.local_addr().map(|a| a.port()).unwrap_or(0);

                            // PROXY protocol (ADR-0022): behind the fly.io edge
                            // the TCP peer is the forwarder, so recover the real
                            // miner IP/port from the v2 header before any
                            // per-connection logic. A missing/invalid header on a
                            // proxy_protocol listener is a hard reject.
                            let (real_ip, remote_port) = if self.config.proxy_protocol {
                                match read_proxy_v2_source(&mut stream).await {
                                    Ok(src) => (src.ip(), src.port()),
                                    Err(e) => {
                                        warn!("[CONNECTION] PROXY protocol parse failed from {}: {}; dropping", addr.ip(), e);
                                        drop(stream);
                                        continue;
                                    }
                                }
                            } else {
                                (addr.ip(), addr.port())
                            };
                            let remote_addr = real_ip.to_string();

                            // Anti-abuse: per-IP connection cap + tracked-IP cap.
                            // Reject before allocating any per-connection state.
                            // Keyed on the real client IP (post-PROXY).
                            let ticket = match self.config.anti_abuse.try_accept_connection(real_ip, std::time::Instant::now()) {
                                Ok(t) => t,
                                Err(rejection) => {
                                    record_anti_abuse_connection_reject(
                                        &self.config.instance_id,
                                        &remote_addr,
                                        rejection.metric_label(),
                                    );
                                    warn!(
                                        "[CONNECTION] anti-abuse rejected accept from {}:{} ({})",
                                        remote_addr, remote_port, rejection
                                    );
                                    drop(stream);
                                    continue;
                                }
                            };

                            debug!("[CONNECTION] new client connecting - {}:{}", remote_addr, remote_port);
                            debug!("[CONNECTION] ===== TCP CONNECTION ESTABLISHED =====");
                            debug!("[CONNECTION] Remote address: {}:{}", remote_addr, remote_port);
                            debug!("[CONNECTION] Local address: {:?}", stream.local_addr());
                            debug!("[CONNECTION] Connection accepted successfully");

                            // Create new MiningState for each client
                            // Each client gets its own isolated state, just like in Go
                            use crate::mining_state::MiningState;
                            let state = Arc::new(MiningState::new());

                            // Clone for logging after move
                            let remote_addr_for_log = remote_addr.clone();
                            let remote_port_for_log = remote_port;

                            debug!("[CONNECTION] Creating StratumContext for {}:{}", remote_addr_for_log, remote_port_for_log);
                            let ctx = StratumContext::new(
                                remote_addr,
                                remote_port,
                                local_port,
                                stream,
                                state,
                                disconnect_tx_clone.clone(),
                            );
                            debug!("[CONNECTION] StratumContext created successfully");

                            debug!("[CONNECTION] Calling on_connect handler");
                            (self.config.on_connect)(ctx.clone());
                            debug!("[CONNECTION] on_connect handler completed");

                            // Spawn client handler
                            debug!("[CONNECTION] Spawning client listener task for {}:{}", remote_addr_for_log, remote_port_for_log);
                            let ctx_clone = ctx.clone();
                            let handler_map = self.config.handler_map.clone();
                            let anti_abuse = Arc::clone(&self.config.anti_abuse);
                            let instance_id = self.config.instance_id.clone();
                            tokio::spawn(async move {
                                debug!("[CONNECTION] Client listener task started for {}:{}", ctx_clone.remote_addr, ctx_clone.remote_port);
                                // `ticket` is dropped at task end (and on every
                                // early `break` below), releasing this IP's
                                // connection slot. The `move` here is what
                                // ties the per-IP slot lifetime to the
                                // connection's listener task.
                                Self::spawn_client_listener(ctx_clone, &handler_map, &anti_abuse, &instance_id).await;
                                drop(ticket);
                                debug!("[CONNECTION] Client listener task ended");
                            });
                            debug!("[CONNECTION] ===== CONNECTION SETUP COMPLETE FOR {}:{} =====", remote_addr_for_log, remote_port_for_log);
                        }
                        Err(e) => {
                            if self.shutting_down.load(std::sync::atomic::Ordering::Acquire) {
                                info!("stopping listening due to server shutdown");
                                break;
                            }
                            error!("[CONNECTION] ===== FAILED TO ACCEPT INCOMING CONNECTION =====");
                            error!("[CONNECTION] Error: {}", e);
                            error!("[CONNECTION] Error kind: {:?}", e.kind());
                            error!("[CONNECTION] Failed to accept connection: {} (kind: {:?})", e, e.kind());
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Spawn a client listener task.
    ///
    /// Anti-abuse hooks:
    /// - Every successfully-parsed frame is gated by the per-IP token
    ///   bucket before dispatch; rate-limited frames disconnect the
    ///   client and bump the `ks_anti_abuse_frame_rate_limited_total`
    ///   counter.
    /// - JSON-RPC parse failures bump `ks_anti_abuse_malformed_frame_total`.
    async fn spawn_client_listener(
        ctx: Arc<StratumContext>,
        handler_map: &Arc<HashMap<String, EventHandler>>,
        anti_abuse: &Arc<AntiAbuseGuard>,
        instance_id: &str,
    ) {
        debug!("[CLIENT_LISTENER] Starting client listener for {}:{}", ctx.remote_addr, ctx.remote_port);
        // Parse remote_addr once. The string was produced by
        // `SocketAddr::ip().to_string()` at accept time, so a parse
        // failure here would mean catastrophic upstream corruption;
        // treat it conservatively by dropping the connection.
        let remote_ip: std::net::IpAddr = match ctx.remote_addr.parse() {
            Ok(ip) => ip,
            Err(e) => {
                error!("[CLIENT_LISTENER] could not re-parse remote_addr `{}` as IpAddr ({e}); disconnecting", ctx.remote_addr);
                ctx.disconnect();
                return;
            }
        };
        let mut buffer = [0u8; 1024];
        let mut line_buffer = String::new();
        let mut first_message = true;

        loop {
            // Check if disconnected
            if !ctx.connected() {
                debug!("[CLIENT_LISTENER] Client {}:{} disconnected", ctx.remote_addr, ctx.remote_port);
                break;
            }

            // Get read half for reading (must drop guard before await)
            let read_half_opt = {
                let mut read_guard = ctx.get_read_half();
                read_guard.take()
            };

            let read_result = if let Some(mut read_half) = read_half_opt {
                // Set read deadline
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

                let result = tokio::time::timeout_at(deadline, read_half.read(&mut buffer)).await;

                // Put read half back
                {
                    let mut read_guard = ctx.get_read_half();
                    *read_guard = Some(read_half);
                }

                result
            } else {
                // Read half is None, disconnect
                warn!("[CONNECTION] Read half is None for {}, disconnecting", ctx.remote_addr);
                break;
            };

            match read_result {
                Ok(Ok(0)) => {
                    // EOF - client closed connection
                    let worker_name = ctx.worker_name.lock().clone();
                    let remote_app = ctx.remote_app.lock().clone();
                    let pending_buffer_bytes = line_buffer.len();
                    let is_pre_handshake = worker_name.is_empty() && remote_app.is_empty();
                    if is_pre_handshake && first_message && pending_buffer_bytes == 0 {
                        debug!(
                            "[CONNECTION] Client {}:{} closed connection (EOF) worker='{}' app='{}' first_message={} pending_buffer_bytes={}",
                            ctx.remote_addr, ctx.remote_port, worker_name, remote_app, first_message, pending_buffer_bytes
                        );
                    } else {
                        info!(
                            "[CONNECTION] Client {}:{} closed connection (EOF) worker='{}' app='{}' first_message={} pending_buffer_bytes={}",
                            ctx.remote_addr, ctx.remote_port, worker_name, remote_app, first_message, pending_buffer_bytes
                        );
                    }
                    break;
                }
                Ok(Ok(n)) => {
                    debug!("[CLIENT_LISTENER] Read {} bytes from {}:{}", n, ctx.remote_addr, ctx.remote_port);

                    // Remove null bytes and process
                    let data: Vec<u8> = buffer[..n].iter().copied().filter(|&b| b != 0).collect();

                    if first_message {
                        let wallet_addr = ctx.wallet_addr.lock().clone();
                        let worker_name = ctx.worker_name.lock().clone();
                        let remote_app = ctx.remote_app.lock().clone();
                        let message_str = String::from_utf8_lossy(&data);

                        // Check for HTTP/2/gRPC protocol in first message (before logging)
                        let first_line = message_str.lines().next().unwrap_or("").trim();
                        if first_line.starts_with("PRI * HTTP/2.0")
                            || first_line.starts_with("PRI * HTTP/2")
                            || first_line == "SM"
                            || first_line.starts_with("GET ")
                            || first_line.starts_with("POST ")
                            || first_line.starts_with("PUT ")
                            || first_line.starts_with("DELETE ")
                            || first_line.starts_with("HEAD ")
                            || first_line.starts_with("OPTIONS ")
                        {
                            error!("{}", LogColors::error("========================================"));
                            error!("{}", LogColors::error("===== PROTOCOL MISMATCH DETECTED (FIRST MESSAGE) ===== "));
                            error!("{}", LogColors::error("========================================"));
                            error!("{} {}", LogColors::error("[ERROR]"), LogColors::label("Client Information:"));
                            error!(
                                "{} {} {}",
                                LogColors::error("[ERROR]"),
                                LogColors::label("  - IP Address:"),
                                format!("{}:{}", ctx.remote_addr, ctx.remote_port)
                            );
                            error!(
                                "{} {} {}",
                                LogColors::error("[ERROR]"),
                                LogColors::label("  - Protocol Detected:"),
                                "HTTP/2 or HTTP (gRPC)"
                            );
                            error!(
                                "{} {} {}",
                                LogColors::error("[ERROR]"),
                                LogColors::label("  - Expected Protocol:"),
                                "Plain TCP/JSON-RPC (Stratum)"
                            );
                            error!(
                                "{} {} {}",
                                LogColors::error("[ERROR]"),
                                LogColors::label("  - First Message (hex):"),
                                hex::encode(&data)
                            );
                            error!(
                                "{} {} {}",
                                LogColors::error("[ERROR]"),
                                LogColors::label("  - First Message (string):"),
                                first_line
                            );
                            error!("{} {}", LogColors::error("[ERROR]"), LogColors::label("Action:"));
                            error!(
                                "{} {}",
                                LogColors::error("[ERROR]"),
                                "  * Rejecting connection - Stratum port only accepts JSON-RPC over plain TCP"
                            );
                            error!(
                                "{} {}",
                                LogColors::error("[ERROR]"),
                                "  * HTTP/2/gRPC connections should use the Kaspa node port (16110), not the bridge port (5555)"
                            );
                            error!("{} {}", LogColors::error("[ERROR]"), "  * Closing connection immediately");
                            error!("{}", LogColors::error("========================================"));

                            // Close connection
                            ctx.disconnect();
                            break;
                        }

                        debug!("{}", LogColors::asic_to_bridge("========================================"));
                        debug!("{}", LogColors::asic_to_bridge("===== FIRST MESSAGE FROM ASIC ===== "));
                        debug!("{}", LogColors::asic_to_bridge("========================================"));
                        debug!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), LogColors::label("Connection Information:"));
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - IP Address:"),
                            format!("{}:{}", ctx.remote_addr, ctx.remote_port)
                        );
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - Wallet Address:"),
                            format!("'{}'", wallet_addr)
                        );
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - Worker Name:"),
                            format!("'{}'", worker_name)
                        );
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - Miner Application:"),
                            format!("'{}'", remote_app)
                        );
                        debug!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), LogColors::label("First Message Data:"));
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - Raw Bytes (hex):"),
                            hex::encode(&data)
                        );
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - Raw Bytes Length:"),
                            format!("{} bytes", data.len())
                        );
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - Message as String:"),
                            message_str
                        );
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - String Length:"),
                            format!("{} characters", message_str.len())
                        );
                        debug!(
                            "{} {} {}",
                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                            LogColors::label("  - String Length:"),
                            format!("{} bytes (UTF-8)", message_str.len())
                        );
                        // Show byte-by-byte breakdown for first 100 bytes
                        if data.len() <= 100 {
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - Byte Breakdown:"),
                                format!("{:?}", data)
                            );
                        } else {
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - First 100 Bytes:"),
                                format!("{:?}", &data[..100.min(data.len())])
                            );
                        }
                        debug!("{}", LogColors::asic_to_bridge("========================================"));
                        first_message = false;
                    }

                    line_buffer.push_str(&String::from_utf8_lossy(&data));

                    // Process complete lines
                    while let Some(newline_pos) = line_buffer.find('\n') {
                        let line = line_buffer[..newline_pos].trim().to_string();
                        line_buffer = line_buffer[newline_pos + 1..].to_string();

                        if !line.is_empty() {
                            // Get client context for detailed logging
                            let wallet_addr = ctx.wallet_addr.lock().clone();
                            let worker_name = ctx.worker_name.lock().clone();
                            let remote_app = ctx.remote_app.lock().clone();

                            // Detect HTTP/2/gRPC connections early and reject them
                            // HTTP/2 connection preface starts with "PRI * HTTP/2.0"
                            if line.starts_with("PRI * HTTP/2.0")
                                || line.starts_with("PRI * HTTP/2")
                                || line == "SM"
                                || line.starts_with("GET ")
                                || line.starts_with("POST ")
                                || line.starts_with("PUT ")
                                || line.starts_with("DELETE ")
                                || line.starts_with("HEAD ")
                                || line.starts_with("OPTIONS ")
                            {
                                error!("{}", LogColors::error("========================================"));
                                error!("{}", LogColors::error("===== PROTOCOL MISMATCH DETECTED ===== "));
                                error!("{}", LogColors::error("========================================"));
                                error!("{} {}", LogColors::error("[ERROR]"), LogColors::label("Client Information:"));
                                error!(
                                    "{} {} {}",
                                    LogColors::error("[ERROR]"),
                                    LogColors::label("  - IP Address:"),
                                    format!("{}:{}", ctx.remote_addr, ctx.remote_port)
                                );
                                error!(
                                    "{} {} {}",
                                    LogColors::error("[ERROR]"),
                                    LogColors::label("  - Protocol Detected:"),
                                    "HTTP/2 or HTTP (gRPC)"
                                );
                                error!(
                                    "{} {} {}",
                                    LogColors::error("[ERROR]"),
                                    LogColors::label("  - Expected Protocol:"),
                                    "Plain TCP/JSON-RPC (Stratum)"
                                );
                                error!("{} {} {}", LogColors::error("[ERROR]"), LogColors::label("  - Received Message:"), &line);
                                error!("{} {}", LogColors::error("[ERROR]"), LogColors::label("Action:"));
                                error!(
                                    "{} {}",
                                    LogColors::error("[ERROR]"),
                                    "  * Rejecting connection - Stratum port only accepts JSON-RPC over plain TCP"
                                );
                                error!(
                                    "{} {}",
                                    LogColors::error("[ERROR]"),
                                    "  * HTTP/2/gRPC connections should use the Kaspa node port (16110), not the bridge port (5555)"
                                );
                                error!("{} {}", LogColors::error("[ERROR]"), "  * Closing connection immediately");
                                error!("{}", LogColors::error("========================================"));

                                // Close connection
                                ctx.disconnect();
                                break;
                            }

                            // Log raw incoming message from ASIC at DEBUG level (verbose details)
                            debug!("{}", LogColors::asic_to_bridge("========================================"));
                            debug!("{}", LogColors::asic_to_bridge("===== RECEIVED MESSAGE FROM ASIC ===== "));
                            debug!("{}", LogColors::asic_to_bridge("========================================"));
                            debug!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), LogColors::label("Client Information:"));
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - IP Address:"),
                                format!("{}:{}", ctx.remote_addr, ctx.remote_port)
                            );
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - Wallet Address:"),
                                format!("'{}'", wallet_addr)
                            );
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - Worker Name:"),
                                format!("'{}'", worker_name)
                            );
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - Miner Application:"),
                                format!("'{}'", remote_app)
                            );
                            debug!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), LogColors::label("Raw Message Data:"));
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - Raw Message:"),
                                line
                            );
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - Message Length:"),
                                format!("{} bytes", line.len())
                            );
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - Message Length:"),
                                format!("{} characters", line.chars().count())
                            );
                            debug!(
                                "{} {} {}",
                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                LogColors::label("  - Raw Bytes (hex):"),
                                hex::encode(line.as_bytes())
                            );

                            // Anti-abuse: token-bucket frame rate limit.
                            // Counted against the IP regardless of whether
                            // the payload is well-formed; an attacker who
                            // floods malformed frames burns the same bucket
                            // as one who floods valid frames.
                            if !anti_abuse.try_consume_frame(remote_ip, std::time::Instant::now()) {
                                record_anti_abuse_frame_limited(instance_id, &ctx.remote_addr);
                                warn!("[CONNECTION] anti-abuse rate-limited {}:{}; disconnecting", ctx.remote_addr, ctx.remote_port);
                                ctx.disconnect();
                                break;
                            }

                            match crate::jsonrpc_event::unmarshal_event(&line) {
                                Ok(event) => {
                                    let params_str = serde_json::to_string(&event.params).unwrap_or_else(|_| "[]".to_string());

                                    // Log parsed event details at DEBUG level (detailed logs moved to debug)
                                    debug!("{}", LogColors::asic_to_bridge("===== PARSING SUCCESSFUL ===== "));
                                    debug!(
                                        "{} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("Parsed Event Structure:")
                                    );
                                    debug!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Method:"),
                                        format!("'{}'", event.method)
                                    );
                                    debug!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Event ID:"),
                                        format!("{:?}", event.id)
                                    );
                                    debug!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - JSON-RPC Version:"),
                                        format!("'{}'", event.jsonrpc)
                                    );
                                    debug!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), LogColors::label("Parameters:"));
                                    debug!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Params Count:"),
                                        event.params.len()
                                    );
                                    debug!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Params JSON:"),
                                        params_str
                                    );
                                    debug!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Params Length:"),
                                        format!("{} characters", params_str.len())
                                    );
                                    // Log each param individually with type information
                                    for (idx, param) in event.params.iter().enumerate() {
                                        let param_str = serde_json::to_string(param).unwrap_or_else(|_| "N/A".to_string());
                                        let param_type = if param.is_string() {
                                            let s = param.as_str().unwrap_or("");
                                            format!("String (length: {}, value: '{}')", s.len(), s)
                                        } else if param.is_number() {
                                            format!("Number (value: {})", param)
                                        } else if param.is_array() {
                                            let arr = param.as_array().unwrap();
                                            format!(
                                                "Array (length: {}, items: {:?})",
                                                arr.len(),
                                                arr.iter()
                                                    .take(5)
                                                    .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "?".to_string()))
                                                    .collect::<Vec<_>>()
                                            )
                                        } else if param.is_object() {
                                            "Object".to_string()
                                        } else if param.is_boolean() {
                                            format!("Boolean (value: {})", param.as_bool().unwrap_or(false))
                                        } else {
                                            "Null".to_string()
                                        };
                                        debug!(
                                            "{} {} {}",
                                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                            LogColors::label(&format!("  - Param[{}]:", idx)),
                                            format!("{} (type: {})", param_str, param_type)
                                        );
                                    }

                                    if let Some(handler) = handler_map.get(&event.method) {
                                        debug!("{}", LogColors::asic_to_bridge("===== PROCESSING MESSAGE ===== "));
                                        debug!(
                                            "{} {} {}",
                                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                            LogColors::label("  - Handler Found:"),
                                            "YES"
                                        );
                                        debug!(
                                            "{} {} {}",
                                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                            LogColors::label("  - Method:"),
                                            format!("'{}'", event.method)
                                        );
                                        debug!(
                                            "{} {}",
                                            LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                            "  - Starting handler execution..."
                                        );
                                        if let Err(e) = handler(ctx.clone(), event).await {
                                            let error_msg = e.to_string();
                                            if error_msg.contains("stale") || error_msg.contains("job does not exist") {
                                                // Log stale job errors as debug (expected behavior, not important)
                                                debug!("{}", LogColors::asic_to_bridge("===== HANDLER EXECUTION RESULT ===== "));
                                                debug!(
                                                    "{} {} {}",
                                                    LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                                    LogColors::validation("  - Result:"),
                                                    "STALE JOB (expected - job no longer exists)"
                                                );
                                                debug!(
                                                    "{} {} {}",
                                                    LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                                    LogColors::label("  - Error Message:"),
                                                    error_msg
                                                );
                                            } else if error_msg.contains("job id is not parsable") {
                                                // Log parsing errors as warnings
                                                warn!(
                                                    "{} {} {}",
                                                    LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                                    LogColors::error("  - Result:"),
                                                    "ERROR (job ID parsing failed)"
                                                );
                                                warn!(
                                                    "{} {} {}",
                                                    LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                                    LogColors::label("  - Error Message:"),
                                                    error_msg
                                                );
                                            } else {
                                                error!(
                                                    "{} {} {}",
                                                    LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                                    LogColors::error("  - Result:"),
                                                    "ERROR (handler execution failed)"
                                                );
                                                error!(
                                                    "{} {} {}",
                                                    LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                                    LogColors::label("  - Error Message:"),
                                                    error_msg
                                                );
                                            }
                                        } else {
                                            debug!("{}", LogColors::asic_to_bridge("===== HANDLER EXECUTION RESULT ===== "));
                                            debug!(
                                                "{} {} {}",
                                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                                LogColors::label("  - Result:"),
                                                "SUCCESS"
                                            );
                                            debug!(
                                                "{} {}",
                                                LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                                "  - Message processed successfully"
                                            );
                                        }
                                        debug!("{}", LogColors::asic_to_bridge("========================================"));
                                    }
                                }
                                Err(e) => {
                                    record_malformed_frame(instance_id, &ctx.remote_addr);
                                    error!("{}", LogColors::asic_to_bridge("========================================"));
                                    error!("{}", LogColors::error("===== ERROR PARSING MESSAGE ===== "));
                                    error!("{}", LogColors::asic_to_bridge("========================================"));
                                    error!(
                                        "{} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("Client Information:")
                                    );
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - IP Address:"),
                                        format!("{}:{}", ctx.remote_addr, ctx.remote_port)
                                    );
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Wallet Address:"),
                                        format!("'{}'", wallet_addr)
                                    );
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Worker Name:"),
                                        format!("'{}'", worker_name)
                                    );
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Miner Application:"),
                                        format!("'{}'", remote_app)
                                    );
                                    error!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), LogColors::label("Failed Message:"));
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Raw Message:"),
                                        line
                                    );
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Message Length:"),
                                        format!("{} bytes", line.len())
                                    );
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Raw Bytes (hex):"),
                                        hex::encode(line.as_bytes())
                                    );
                                    error!(
                                        "{} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("Parse Error Details:")
                                    );
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Error Type:"),
                                        "JSON Parsing Failed"
                                    );
                                    error!(
                                        "{} {} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::error("  - Error Message:"),
                                        e
                                    );
                                    error!(
                                        "{} {}",
                                        LogColors::asic_to_bridge("[ASIC->BRIDGE]"),
                                        LogColors::label("  - Possible Causes:")
                                    );
                                    error!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), "    * Malformed JSON syntax");
                                    error!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), "    * Protocol mismatch");
                                    error!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), "    * Incomplete message");
                                    error!("{} {}", LogColors::asic_to_bridge("[ASIC->BRIDGE]"), "    * Encoding issue");
                                    error!("{}", LogColors::asic_to_bridge("========================================"));
                                }
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    // Check if it's a connection closed error (expected when client disconnects)
                    let error_msg = e.to_string();
                    if error_msg.contains("forcibly closed")
                        || error_msg.contains("Connection reset")
                        || error_msg.contains("Broken pipe")
                        || e.kind() == std::io::ErrorKind::ConnectionReset
                        || e.kind() == std::io::ErrorKind::BrokenPipe
                    {
                        let worker_name = ctx.worker_name.lock().clone();
                        let remote_app = ctx.remote_app.lock().clone();
                        let is_pre_handshake = worker_name.is_empty() && remote_app.is_empty();
                        if is_pre_handshake {
                            debug!(
                                "[CONNECTION] Client {}:{} disconnected (reset/broken pipe) kind={:?} worker='{}' app='{}' msg='{}'",
                                ctx.remote_addr,
                                ctx.remote_port,
                                e.kind(),
                                worker_name,
                                remote_app,
                                error_msg
                            );
                        } else {
                            info!(
                                "[CONNECTION] Client {}:{} disconnected (reset/broken pipe) kind={:?} worker='{}' app='{}' msg='{}'",
                                ctx.remote_addr,
                                ctx.remote_port,
                                e.kind(),
                                worker_name,
                                remote_app,
                                error_msg
                            );
                        }
                    } else {
                        error!("error reading from socket: {}", e);
                    }
                    break;
                }
                Err(_) => {
                    // Timeout - continue
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    continue;
                }
            }
        }

        // Tear the socket down, then route through the disconnect handler
        // so `ClientHandler::on_disconnect` runs (Prometheus disconnect
        // accounting + the `SessionClosed` event feeding the firmware
        // breakdown, ADR-0023). `disconnect()` is idempotent, so the
        // handler's own call is a no-op; identity (worker/app) remains
        // readable for the event.
        ctx.disconnect();
        ctx.notify_disconnect();
    }

    /// Handle an event
    pub fn handle_event(
        &self,
        _ctx: Arc<StratumContext>,
        event: JsonRpcEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_handler) = self.config.handler_map.get(&event.method) {
            // Note: This is a sync wrapper - actual handlers should be async
            // For now, we'll handle this in spawn_client_listener
            Ok(())
        } else {
            Ok(())
        }
    }
}

/// Read and parse a PROXY protocol **v2** header from the front of a
/// freshly-accepted stream, returning the real client source address
/// (ADR-0022). The fly.io edge is configured for v2
/// (`proxy_proto_options = { version = "v2" }`), which is
/// length-delimited, so we read exactly the header bytes and leave the
/// stratum payload untouched in the socket — no buffer injection needed.
///
/// A 5s deadline bounds a stalled/malicious peer; the header normally
/// arrives in the first segment alongside the connection.
async fn read_proxy_v2_source<R: tokio::io::AsyncRead + Unpin>(stream: &mut R) -> Result<std::net::SocketAddr, String> {
    use std::net::{IpAddr, SocketAddr};

    // PROXY v2 12-byte signature, then ver/cmd, fam/proto, and a 2-byte
    // big-endian length of the address+TLV block that follows.
    const SIG: [u8; 12] = [0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A];

    let read = async {
        let mut fixed = [0u8; 16];
        stream.read_exact(&mut fixed).await.map_err(|e| format!("read v2 fixed header: {e}"))?;
        if fixed[..12] != SIG {
            return Err("not a PROXY protocol v2 header".to_string());
        }
        let addr_len = u16::from_be_bytes([fixed[14], fixed[15]]) as usize;
        let mut buf = vec![0u8; 16 + addr_len];
        buf[..16].copy_from_slice(&fixed);
        stream.read_exact(&mut buf[16..]).await.map_err(|e| format!("read v2 address block: {e}"))?;

        let header = ppp::v2::Header::try_from(buf.as_slice()).map_err(|e| format!("parse v2 header: {e:?}"))?;
        match header.addresses {
            ppp::v2::Addresses::IPv4(a) => Ok(SocketAddr::new(IpAddr::V4(a.source_address), a.source_port)),
            ppp::v2::Addresses::IPv6(a) => Ok(SocketAddr::new(IpAddr::V6(a.source_address), a.source_port)),
            other => Err(format!("unsupported PROXY address family: {other:?}")),
        }
    };

    match tokio::time::timeout(std::time::Duration::from_secs(5), read).await {
        Ok(result) => result,
        Err(_) => Err("timed out reading PROXY protocol header".to_string()),
    }
}

#[cfg(test)]
mod proxy_protocol_tests {
    use super::read_proxy_v2_source;
    use ppp::v2::{Builder, Command, IPv4, IPv6, Protocol, Version};

    /// A valid v2 IPv4 header yields the source address, and only the
    /// header bytes are consumed — the trailing stratum bytes remain.
    #[tokio::test]
    async fn parses_ipv4_source_and_leaves_payload() {
        let header = Builder::with_addresses(
            Version::Two | Command::Proxy,
            Protocol::Stream,
            IPv4::new([203, 0, 113, 5], [10, 0, 0, 1], 49_321, 7777),
        )
        .build()
        .unwrap();

        let payload = b"{\"id\":1,\"method\":\"mining.subscribe\"}\n";
        let mut stream: Vec<u8> = header.clone();
        stream.extend_from_slice(payload);

        let mut cursor = stream.as_slice();
        let src = read_proxy_v2_source(&mut cursor).await.unwrap();

        assert_eq!(src.ip().to_string(), "203.0.113.5");
        assert_eq!(src.port(), 49_321);
        // The reader stopped exactly at the header boundary.
        assert_eq!(cursor, payload);
    }

    #[tokio::test]
    async fn parses_ipv6_source() {
        let header = Builder::with_addresses(
            Version::Two | Command::Proxy,
            Protocol::Stream,
            IPv6::new([0x2001, 0xdb8, 0, 0, 0, 0, 0, 0x42], [0xfe80, 0, 0, 0, 0, 0, 0, 1], 51_000, 8888),
        )
        .build()
        .unwrap();

        let mut cursor = header.as_slice();
        let src = read_proxy_v2_source(&mut cursor).await.unwrap();
        assert_eq!(src.ip().to_string(), "2001:db8::42");
        assert_eq!(src.port(), 51_000);
    }

    /// A stream that does not begin with the v2 signature is rejected
    /// (e.g. a miner that connected directly without the forwarder).
    #[tokio::test]
    async fn rejects_non_proxy_stream() {
        let mut cursor = b"{\"id\":1,\"method\":\"mining.subscribe\"}\n".as_slice();
        assert!(read_proxy_v2_source(&mut cursor).await.is_err());
    }
}
