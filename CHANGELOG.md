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
- Phase 2 milestone 3 (prep): seven additional repository aggregates
  to complete the schema's query surface ahead of the legacy
  importer. New modules:
    - `repo::pool_meta` — single-row key/value store; `get` /
      idempotent `set` with `updated_at` refresh
    - `repo::connection_session` — per-stratum-TCP-session record;
      `open` / `bind_worker` / `close` / `increment_counters` /
      `list_for_worker`. Maps the postgres `INET` column to
      `String` at the Rust boundary (no `ipnetwork` dep)
    - `repo::treasury` — periodic hot-wallet snapshots; `insert`
      / `latest` / `list_recent`
    - `repo::share_window` — pre-aggregated PROP rollups for
      closed DAA windows; `insert` / `find` / `list_for_window`
      with the schema's UNIQUE-window guard
    - `repo::share_allocation` — per-wallet PROP allocation of a
      block's matured reward. `NewAllocation::is_balanced` does
      client-side rejection of unbalanced rows before the DB
      CHECK fires; `insert_batch` flattens per-wallet vectors
      via `UNNEST` in one round-trip; aggregate
      `pending_balance_for_wallet` for the accountant's
      planned-payout query
    - `repo::nacho_rebate` — running NACHO rebate balance per
      wallet; `accrue` / `mark_paid` / `list_pending` with
      `paid <= accrued` enforcement and a `pending_sompi()`
      derived getter
    - `repo::payout` — payout-cycle / payout / KRC-20 transfer
      triple. Idempotency-key composer
      (`kas-<daa_start>-<daa_end>`, `krc20-<daa_start>-<daa_end>`),
      cycle lifecycle helpers (broadcasting / partially-settled /
      settled / failed), per-recipient payout lifecycle
      (submit-with-tx-hash / confirmed / failed-with-reason),
      KRC-20 commit/reveal state machine
  Twenty-five new integration tests in
  `crates/katpool-db/tests/repo_payouts.rs` cover idempotency,
  lifecycle transitions, DB-CHECK enforcement (NACHO `paid > accrued`,
  share-allocation balance equation, payout uniqueness), the
  `NewAllocation` client-side balance guard, and the
  `(idempotency_key)` format stability. Workspace test count grows
  from 108 to 133.
- Phase 2 milestone 2: repository layer over the schema introduced
  in milestone 1. Free functions on `impl sqlx::PgExecutor<'_>`
  organised by aggregate — works with both `&PgPool` for
  single-statement contexts and `&mut Transaction` (via `&mut *tx`)
  for atomic multi-statement work. Strongly-typed ID newtypes
  (`WalletId`, `WorkerId`, `SessionId`, `ShareId`, `BlockId`,
  `AuditLogId`) prevent confusion between table identities at the
  type level. Aggregates shipped:
    - `repo::wallet` — `ensure` (upsert by address with
      `last_seen_at` refresh), `find_by_address`, `get_by_id`
    - `repo::worker` — `ensure`, `get_by_id`, `list_for_wallet`
    - `repo::share` — `insert_credited` (the hot-path call from
      the accountant's `PoolEvent::ShareCredited` handler),
      `sum_weight_for_window` / `count_for_window` /
      `total_weight_for_window` for PROP allocation reads
    - `repo::block` — `insert`, `find_by_hash`, lifecycle
      transitions (`mark_submitted`, `mark_confirmed_blue`,
      `mark_matured`, `mark_orphaned`) with idempotency, plus
      `list_by_status` for operator views
    - `repo::audit` — append-only log via the `NewEntry` builder
      with subject/correlation-id wiring, `list_for_subject`
  Validated newtypes from `katpool-domain` (`WalletAddress`,
  `WorkerName`, `BlockHash`, `DaaScore`, `ShareDifficulty`,
  `CorrelationId`) are the public API; the domain invariants flow
  into the database boundary unchanged. Seventeen new integration
  tests in `crates/katpool-db/tests/repo.rs` exercise idempotency,
  cascade behaviour, lifecycle CHECK enforcement, transaction
  rollback semantics, and the per-aggregate query contracts against
  a real Postgres testcontainer. Workspace gains
  `serde_json` as a declared dep on `katpool-db`. Two follow-up
  issues opened proactively: #8 (`gh pr edit --add-label` chokes on
  Projects-classic deprecation; REST workaround documented) and #9
  (pin `testcontainers` postgres image to match production
  `postgres:17`).
- Phase 2 milestone 1: `katpool-db` crate with the full schema for the
  rebuild — 14 tables (`wallet`, `worker`, `connection_session`,
  `share`, `share_window`, `block`, `share_allocation`, `payout_cycle`,
  `payout`, `nacho_rebate_accrual`, `krc20_pending_transfer`,
  `treasury_snapshot`, `audit_log`, `pool_meta`), 5 enum state-machines
  (`block_status`, `payout_kind`, `payout_cycle_status`,
  `payout_status`, `krc20_transfer_status`), foreign-key integrity
  throughout, CHECK constraints rejecting bad-shape data at the storage
  layer (wallet-address format per network, balance equation in
  `share_allocation`, lifecycle ordering in `block` and `payout`,
  uniqueness on `payout (cycle_id, wallet_id)` for payout idempotency).
  Connection pool builder with operator-tunable
  `KATPOOL_DB_*` env vars (mirrors the bridge's anti-abuse config
  pattern); typed `DbError` with `is_transient` / `is_not_found` /
  `sqlstate()` classification helpers; embedded `sqlx::migrate!`
  migrator that fail-closes on schema-ahead-of-binary. Twelve unit
  tests cover `PoolConfig`/`DbError`; twelve integration tests spin
  up an ephemeral postgres via `testcontainers-modules` and assert
  every documented table, enum, FK cascade, CHECK constraint, and
  idempotency invariant works end-to-end. New
  `docs/decisions/0011-db-schema-and-migrations.md` documenting the
  schema rationale and migration strategy (no down-migrations;
  rollback via pgBackRest restore from ADR-0007). New
  `docs/db-schema.md` operator reference with ER diagram and worked
  query examples per table. Workspace gains
  `testcontainers-modules` (with the `postgres` feature) and
  `kaspa-math` as declared deps.
- Phase 1 closeout: `bridge/examples/cpu_stratum_miner.rs` — a
  self-contained stratum-protocol CPU miner (~250 LOC) using the
  workspace-pinned `kaspa_pow::matrix::Matrix` + `kaspa_hashes::PowHash`
  for PoW, raw line-delimited JSON-RPC for the wire protocol, and a
  thread-striped nonce search across all available CPU cores. The
  public ecosystem has no maintained Crescendo + Toccata-aware CPU
  stratum miner (`kaspanet/cpuminer` v0.2.7 and `elichai/kaspa-miner`
  are both solo gRPC miners that bypass any stratum layer), so this
  artifact is required for end-to-end stratum smoke runs in CI.
  Companion bridge example `bridge/examples/gen_testnet_addr.rs`
  generates a valid bech32 `kaspatest:` address via
  `kaspa_addresses::Address::new` with `/dev/urandom`-seeded payload
  — used by the smoke harness's `--wallet` argument.
  `kaspa-math` added to workspace dependencies (already a transitive
  dep, now declared so the example can call `Uint256::from_le_bytes`
  directly).
  Empirical finding from running the smoke against the operator's
  Toccata-aware kaspad-tn10 at `193.26.159.181:16210`: bridge boot
  in **503 ms**, ≥ **184 mining.notify** delivered in 60 s,
  **38M PoW hashes** computed by the CPU miner, zero panics in either
  process. The Phase 1 acceptance row 12/13 volume threshold (≥ 100
  shares, ≥ 1 block in 60 s) is **mathematically out of reach for any
  CPU stratum miner at the bridge's u32 minimum pool difficulty** and
  is deferred to the Phase 7 cutover smoke with real ASIC hash. Phase
  1 acceptance now records pipeline-GREEN at CPU scale and volume-
  GREEN at ASIC scale (deferred). See
  `docs/phase-1-acceptance.md` "CPU-mining empirical limit" block.
- Phase 1 infra: dedicated Toccata-aware testnet-10 kaspad node
  co-resident with the existing dockerized mainnet kaspad on the
  pool VPS. New hardened systemd unit
  `ops/kaspad/katpool-kaspad-tn10.service` (systemd-analyze security
  exposure level **1.2 OK**), idempotent installer
  `ops/kaspad/install-kaspad-tn10.sh` that downloads the upstream
  `tn10-toc2` release zip pinned by SHA-256
  (`b1664d7336b7b536f98a7383ada6bffec71df7fc0d017f54fd4ec2434d7c5f44`),
  dedicated `kaspad-tn10` system user, data dir at
  `/var/lib/kaspad-tn10/data`, ports 16210 (gRPC) / 16211 (P2P) /
  17210 (wRPC-borsh) / 18210 (wRPC-json). The legacy mainnet
  kaspad (v1.0.1 in docker, 128 GB data dir) is left untouched per
  ADR-0010. Phase 1 acceptance row 11 (boot time) measured at
  **503 ms** against the operator's Toccata-aware external node,
  well under the 5-second budget. New ADR-0010
  (`docs/decisions/0010-multi-tenant-kaspad-on-pool-vps.md`)
  documents the multi-tenant strategy, the Toccata constraint
  (vendored `kaspa-*` v1.1.0 crates predate Toccata, so the bridge
  must run external-only against testnet-10), and the explicit
  deferral of mainnet-migration to Phase 7. New runbook 13
  (`docs/runbooks/13-kaspad-tn10-bootstrap.md`) covers install /
  upgrade / incident-recovery procedures. Capacity plan updated to
  reflect the new third-tenant footprint (~30 GB disk, ~5 GiB RAM,
  1–2 vCPU; total saturated still leaves >65% headroom). New bridge
  example `bridge/examples/gen_testnet_addr.rs` produces a valid
  bech32 `kaspatest:` address using `kaspa_addresses::Address::new`
  with cryptographic randomness from `/dev/urandom`, used by the
  acceptance smoke harness for the wallet field.
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
