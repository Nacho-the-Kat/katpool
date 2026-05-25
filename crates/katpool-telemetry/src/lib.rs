//! Telemetry wiring: tracing-subscriber JSON output, OpenTelemetry OTLP
//! export, correlation-id propagation across async tasks.
//!
//! Provides a single `init(config)` function that every binary calls before
//! anything else. Failure to call it is a compile-time error wherever
//! reasonable; otherwise a runtime panic at startup.
//!
//! Implemented in Phase 1 alongside the bridge fork.

#![cfg_attr(not(test), warn(missing_docs))]

/// Crate version constant.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
