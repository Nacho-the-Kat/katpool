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
