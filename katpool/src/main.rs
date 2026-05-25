//! katpool — main wiring binary.
//!
//! Phase 0 stub. The real wiring (load config, init telemetry, init DB,
//! start bridge, start accountant, start payout schedulers, start API,
//! handle graceful shutdown) lands across Phases 1-6.

#![cfg_attr(not(test), warn(missing_docs))]

/// Process entrypoint. In Phase 0 this just prints version banners for
/// every linked crate to prove the workspace links correctly, then exits.
fn main() {
    // Single allowed `println!` outside of `tracing`. This is the entrypoint
    // before telemetry is initialised. All real output once we wire telemetry
    // (Phase 1) goes through `tracing`.
    #[allow(clippy::print_stdout)]
    {
        println!("katpool v{}", env!("CARGO_PKG_VERSION"));
        println!("  katpool-domain         v{}", katpool_domain::VERSION);
        println!("  katpool-config         v{}", katpool_config::VERSION);
        println!("  katpool-db             v{}", katpool_db::VERSION);
        println!("  katpool-metrics        v{}", katpool_metrics::VERSION);
        println!("  katpool-telemetry      v{}", katpool_telemetry::VERSION);
        println!("  katpool-secrets        v{}", katpool_secrets::VERSION);
        println!("  accountant             v{}", accountant::VERSION);
        println!("  payout-kas             v{}", payout_kas::VERSION);
        println!("  payout-krc20           v{}", payout_krc20::VERSION);
        println!("  api                    v{}", api::VERSION);
    }
}
