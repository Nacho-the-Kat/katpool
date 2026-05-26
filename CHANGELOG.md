# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Pre-1.0.0 releases are development snapshots and may change in
backward-incompatible ways at every minor bump.

## [Unreleased]

### Added

- Phase 0 milestone 1: cargo workspace bootstrap with 14 crates pinning
  Rust 1.88 and edition 2024, strict workspace-wide lint configuration
  (forbid `unsafe_code`; deny `unwrap` / `expect` / `panic` / `indexing` /
  `float_arithmetic` / `print_stdout` / `print_stderr` / `todo` /
  `unimplemented` / `dbg_macro` / `integer_division`).
- Phase 0 milestone 2: `rustfmt.toml`, `clippy.toml`, and `deny.toml`
  enforcing 100-column lines, MSRV-aware clippy, strict licence allowlist
  (Apache-2.0 / MIT / ISC / BSD-2/3 / Unicode-3.0 / Zlib / CDLA-Permissive-2.0),
  `unknown-registry = deny`, `unknown-git = deny`, ban list for
  openssl/native-tls/git2/actix-web with redirects, controlled
  `skip-tree` for the sqlx, opentelemetry, rand, and config families.
- Phase 0 milestone 3: repository governance (this changelog, `SECURITY.md`,
  `README.md`, dual `LICENSE-MIT` and `LICENSE-APACHE`, root `CODEOWNERS`,
  pull-request and issue templates, `.github/branch-protection.md`
  documenting the required `main`-branch settings).
- Phase 0 milestone 4: documentation scaffold. Authoritative
  references for `architecture.md`, `threat-model.md` (STRIDE),
  `custody.md` (sops/age + OS-level isolation operational design),
  `kips.md` (KIP-9 and KIP-13 implementation reference), `capacity-plan.md`
  (measured NetCup specs, budgets, sizing triggers), `onboarding.md`,
  `cutover-plan.md`. Nine ADRs (MADR 4.0 format) cover every Phase 0
  architectural decision. Eleven runbooks cover the named incident
  classes with a uniform Symptom / Confirm / Diagnose / Remediate /
  Verify / Post-incident structure.
- Phase 0 milestone 5: CI workflows. `ci.yml` runs fmt, clippy
  (`-D warnings`), test, cargo-deny, cargo-audit, cargo doc, and
  cargo-tarpaulin coverage on every push and PR. `release.yml`
  builds a static musl binary, generates a CycloneDX SBOM via
  syft, signs both via cosign keyless (OIDC), and publishes a
  draft GitHub release. `security.yml` runs weekly cargo-audit,
  cargo-deny, and Trivy filesystem scans. Every third-party
  action is pinned by full commit SHA with a trailing comment
  naming the human-readable tag.
- Phase 1 milestone 1: vendored rusty-kaspa v1.1.0 stratum bridge
  under `bridge/`, with a documented local-divergence register
  (`bridge/UPSTREAM.md`), per-directory `rustfmt.toml` matching
  upstream style, bridge-local lint overrides, dual workspace
  build (our strict pedantic-and-nursery rules for new crates,
  upstream-tolerant for the vendored bridge), and re-vendoring
  procedure documented for future v1.x bumps.
- Phase 1 milestone 4 (Phase 1 close-out): operator-tunable
  anti-abuse limits via `KATPOOL_ANTI_ABUSE_*` environment variables
  (`MAX_CONN_PER_IP`, `MAX_TRACKED_IPS`, `FRAME_RATE_PER_SEC`,
  `FRAME_BURST`). Malformed values are fail-fast at start-up so an
  operator typo never silently degrades protection. Pure
  closure-injected `AntiAbuseConfig::from_lookup` with five
  deterministic tests plus an `AntiAbuseConfig::from_env` thin
  wrapper. Hardened systemd unit at `ops/systemd/katpool-bridge.service`
  passing `systemd-analyze security` with exposure level **1.1 OK**
  (NoNewPrivileges, ProtectSystem=strict, ProtectHome,
  ProtectKernel{Tunables,Modules,Logs}, ProtectControlGroups,
  PrivateTmp/Devices/Mounts, LockPersonality,
  MemoryDenyWriteExecute, RestrictAddressFamilies, RestrictNamespaces,
  CapabilityBoundingSet emptied, SystemCallFilter `@system-service`
  minus `@privileged @resources @raw-io @reboot @swap @cpu-emulation`,
  RemoveIPC, RestrictSUIDSGID, ProtectProc=invisible, ProcSubset=pid,
  IPAddressDeny=any with explicit allow-list drop-in). Two
  `.conf.example` drop-ins for anti-abuse and network tuning,
  idempotent `install.sh`. New testnet-10 acceptance smoke harness
  at `scripts/testnet10-smoke.sh` driving a 60-second CPU-miner run
  and reporting boot time, shares accepted, and blocks mined as JSON
  against the documented Phase 1 thresholds. New
  `docs/runbooks/12-testnet10-smoke.md` runbook and
  `docs/phase-1-acceptance.md` rollup tracking the 14 Phase 1
  acceptance items.
- Phase 1 milestone 3: per-IP anti-abuse layer for the stratum
  listener. New `bridge::anti_abuse::AntiAbuseGuard` enforces a
  connection cap per source IP, a tracked-IP cap (memory safety
  under attack), and a token-bucket frame-rate limit. RAII
  `ConnTicket` releases the per-IP slot on connection drop, so the
  guard cannot leak counts. Time-injected for deterministic unit
  testing; 13 new tests cover validated config, conn-cap, ticket
  release, distinct-IP isolation, tracked-IP cap, burst behaviour,
  refill semantics, untracked-IP rejection, and the unlimited mode.
  Four new Prometheus counters
  (`ks_anti_abuse_connection_reject_total{reason}`,
  `ks_anti_abuse_frame_rate_limited_total`,
  `ks_anti_abuse_malformed_frame_total`,
  `ks_anti_abuse_bad_address_total`) surface every rejection path.
  `handle_authorize` now disconnects on bech32 failure instead of
  merely returning an error to the listener loop.
  Stratum JSON-RPC parser fuzz harness added under `bridge/fuzz/`
  as a non-workspace cargo-fuzz crate (nightly-only because
  libfuzzer-sys requires nightly); local acceptance run on
  2026-05-25 was 1,500,000 iterations in 23 s with zero panics.
- Phase 1 milestone 2: `katpool-domain` types
  (`WalletAddress`, `WorkerName`, `ShareDifficulty`, `DaaScore`,
  `BlockHash`, `CorrelationId`) — every newtype validates at
  construction, returns typed errors, and serialises transparently.
  Defines the `PoolEvent` enum (`ShareCredited`, `ShareRejected`,
  `BlockFound`, `BlockAccepted`) and the `ShareRejectReason`
  taxonomy (`stale`, `low_difficulty`, `bad_pow`, `missing_job`,
  `malformed_frame`, `duplicate_submit`, `bad_address`). The
  stratum bridge's `share_handler.rs` now emits one `PoolEvent`
  per submission outcome and per block lifecycle event on an
  optional `tokio::sync::broadcast` channel injected via
  `ShareHandler::with_event_bus`. Best-effort emission with
  shared per-submission `CorrelationId` for downstream tracing.
  Forty-eight unit tests cover types and emission (42 in
  `katpool-domain`, 6 in the bridge event-bus module).

[Unreleased]: https://github.com/Nacho-the-Kat/katpool/commits/main
