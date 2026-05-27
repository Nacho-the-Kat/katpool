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
- Phase 3 milestone 2 (accountant: window aggregation + reject
  persistence + per-miner stats): the read-side primitives the
  Phase 6 HTTP API will compose, plus the pre-aggregation that
  M3's PROP allocation engine reads instead of scanning the
  live `share` table per block.
    - Migration `20260527000000_share_reject.sql` introduces the
      `share_reject_reason` postgres enum (variants byte-for-
      byte match `ShareRejectReason::as_str()`) and the
      `share_reject` table with three indexes (worker-time,
      wallet-time, reason-time) for the three canonical access
      patterns.
    - `repo::share_reject` — `insert`, `list_for_wallet`,
      `count_by_reason_for_wallet`, `count_by_reason_pool_wide`,
      plus a `TryFrom<ShareRejectReason>` mapping that
      deliberately rejects unknown upstream variants so the
      build fails until a paired migration ships (defends
      against the `#[non_exhaustive]` enum drift).
    - `repo::share_stats` — read-only aggregations:
      `accepted_for_wallet`, `accepted_pool_wide`,
      `hashrate_estimate_for_wallet` / `_pool_wide`
      (`weight * 2^32 / window_secs` convention), and
      `accepted_and_rejected_for_wallet` (one-round-trip
      summary for the `/miner/{addr}` API endpoint).
    - `accountant::WindowAggregator::close_window` — closes a
      half-open `[daa_start, daa_end)` range with a single
      transactional `INSERT ... SELECT ... GROUP BY` over
      `share`, materialising one `share_window` row per
      contributing wallet. Idempotent via the table's
      `UNIQUE (wallet_id, daa_start, daa_end)` plus
      `ON CONFLICT DO UPDATE` that refreshes
      `total_weight` / `share_count` / `ended_at` while
      preserving the original `started_at`.
    - Consumer wires `ShareRejected` → `share_reject` rows in
      addition to the existing metric tick. Unknown-reason
      events still tick the metric but skip the insert.
    - 14 new tests (5 window_aggregator + 4 share_reject + 5
      share_stats), bringing the accountant suite to 33 tests
      (11 unit + 22 integration).
- Phase 3 milestone 1 (accountant scaffold + event ingestion):
  the pool accountant's foundation — event consumer, fee model,
  wallet-tier classification framework. Subsequent Phase 3 PRs
  layer share-window aggregation (M2), PROP allocation (M3), and
  replay-determinism harness (M4) on top.
    - `accountant::EventConsumer` — `tokio::sync::broadcast::Receiver<PoolEvent>`
      consumer that writes `wallet`/`worker`/`share`/`block` rows
      via the repo layer. Handles lag (skip + metric), channel
      close (clean shutdown), per-event errors (log + metric, no
      task death), and BlockFound idempotency via the new
      `repo::block::ensure` helper (`INSERT ... ON CONFLICT (hash)
      DO UPDATE` returning a (`BlockId`, `EnsureOutcome`) pair).
    - `accountant::FeeConfig` — operator-tunable topline fee via
      `KATPOOL_FEE_TOPLINE_BPS` (basis points integer; default 75
      = 0.75%; max 1 000 bps to guard against typos). Pure
      `from_lookup` constructor takes a lookup closure, so tests
      exercise parse/validation without touching process env (the
      workspace forbids `unsafe_code` and edition-2024 `set_var`
      is now unsafe).
    - `accountant::WalletTier` — `Standard` (33% rebate of fee)
      and `Elite` (100% rebate of fee), with rebate ratios fixed
      in code (per ADR-0012). Defined as both Rust enum and
      `sqlx::Type` against a `wallet_tier` postgres enum that
      lands with M3's migration.
    - `accountant::TierClassifier` trait + `StaticTierClassifier`
      stub. HTTP-backed `KasplexTierClassifier` deferred to M3
      where the allocation engine actually needs tier resolution.
      On any classifier error the safe fallback is `Standard`.
    - Prometheus metrics: `ks_accountant_events_total`,
      `ks_accountant_events_lagged_total`,
      `ks_accountant_event_errors_total`,
      `ks_accountant_share_inserts_total`,
      `ks_accountant_block_transitions_total`. Every metric
      carries an `instance` label for primary-vs-shadow
      disambiguation during the Phase 7 shadow-run window.
    - 11 unit tests + 8 integration tests against ephemeral
      Postgres (testcontainers) covering: share path, block
      lifecycle, BlockFound idempotency, orphan BlockAccepted,
      lag tolerance, clean shutdown, share-rejected metric-only
      semantics, and weight aggregation.
    - ADR-0012 (`docs/decisions/0012-fee-model-and-tier-classification.md`)
      capturing the fee model, basis-points env knob, tier-at-
      maturity decision, deferred migration plan, and audit-trail
      column rollout strategy.
- Phase 2 milestone 4 (importer acceptance): scale + property
  tests for the legacy importer, the operator rehearsal wrapper,
  and the Phase 2 acceptance evidence page.
    - `katpool-import-legacy/tests/import_scale.rs` — two
      entry points: `scale_acceptance_ci_default` (1K blocks,
      ~7 s, runs unconditionally) and
      `scale_acceptance_local_rehearsal` (10K blocks, ~50 s,
      `#[ignore]`d for local rehearsal). Both end in a reconcile
      pass and a throughput sentinel that catches regressions
      that would blow the 30-minute cutover budget. Measured
      throughput: ~2.4 ms/block, linear in row count.
    - `katpool-import-legacy/tests/import_properties.rs` — 5
      cross-cutting invariant tests: rerun-with-new-rows
      converges; rebate `set_accrual` overwrites (not
      accumulates); partial-failure restart safety;
      reconcile-detects-legacy-mutation-after-import.
    - `scripts/legacy-importer-rehearsal.sh` — operator wrapper
      script (dry-run by default, `--no-dry-run` for cutover
      hot-run). Captures reconcile JSON, tracing log,
      audit-log snapshot, and a manifest containing git rev +
      binary sha256 into a timestamped artefact directory.
      Required by the cutover ticket.
    - `docs/runbooks/14-legacy-importer.md` updated to recommend
      the rehearsal script as the primary invocation path; the
      raw binary command is now documented as a fallback only.
    - `docs/cutover-plan.md` T-2m step rewritten to reference
      the rehearsal script + the `manifest.reconcile_all_passed`
      gate, replacing the obsolete path it inherited from the
      original plan.
    - `docs/phase-2-acceptance.md` — Phase 2 acceptance matrix
      modelled on the Phase 1 sibling: 12 acceptance rows
      cross-referenced to PRs, scale-run history with
      empirical timings, full check inventory for the
      reconciliation pass.
- Phase 2 milestone 3 (importer, part B): the four remaining
  legacy-table transforms wired into `katpool-import-legacy`,
  plus the cross-table reconciliation pass:
    - `transform::balances` — `miners_balance.nacho_rebate_kas` →
      `nacho_rebate_accrual.accrued_sompi` via the new
      `repo::nacho_rebate::set_accrual` (idempotent SET, distinct
      from the additive `accrue`).
    - `transform::payments` — `payments` rows grouped by
      `transaction_hash` → one `payout_cycle (kind=kas)` per group,
      one `payout` per recipient, idempotent on
      `UNIQUE (cycle_id, wallet_id)`. Synthetic `daa_start=0,
      daa_end=1` because legacy never tracked DAA range; cycles
      identified by `idempotency_key = 'kas-legacy-<tx_hash>'`.
      Cycle is brought to `settled` status atomically.
    - `transform::nacho_payments` — same shape as `payments` but
      with `kind=krc20_nacho` and `idempotency_key =
      'krc20-legacy-<tx_hash>'`. Stores `krc20_commit_hash` +
      `krc20_reveal_hash` (the legacy `transaction_hash` doubles
      as both since legacy didn't split commit/reveal).
    - `transform::krc20` — `pending_krc20_transfers` → singleton
      `payout_cycle` per row + `payout` + `krc20_pending_transfer`,
      with full status mapping (`PENDING`/`COMPLETED`/`FAILED` →
      `pending`/`completed`/`failed`). Failed rows carry a
      `failure_reason` for forensics.
    - `reconcile` — post-import cross-aggregate pass: row counts,
      monetary totals, per-status counts. Importer exits with code
      `2` on any mismatch so CI / runbook scripts can detect
      reconciliation failure without parsing stdout. Reconcile
      runs even in `--dry-run` mode (read-only).
    - Operator runbook ([14-legacy-importer.md](docs/runbooks/14-legacy-importer.md))
      documenting the dry-run / cutover / restart flows.
    - 16 new integration tests against ephemeral Postgres
      (testcontainers), in addition to the 6 existing
      `import_blocks` tests; full importer suite now 26 tests.
- Phase 2 milestone 3 (importer, part A): new
  `katpool-import-legacy` binary crate at the workspace top level
  that walks the previous-generation pool's `katpool_mainnet`
  database and writes into the new schema. This commit ships the
  scaffold and the `block_details` → `(wallet, worker, block)`
  transform, which is the largest single table to migrate
  (production: 539,397 rows). Subsequent commits in this series
  add the remaining transforms (`miners_balance` →
  `nacho_rebate_accrual`; `payments` + `nacho_payments` →
  `payout_cycle` + `payout`; `pending_krc20_transfers` →
  `krc20_pending_transfer`).
  Importer properties:
    - **Idempotent.** Every write goes through an `ON CONFLICT DO
      NOTHING` path or the repo layer's `ensure`-style UPSERT.
      Re-running zero-cost; classified as `skipped` in stats.
    - **Deterministic correlation ids.** UUID v5 derived from the
      block hash (DNS namespace) so audit-log forensics are
      reproducible across re-imports.
    - **Validation-first.** A pure `parse_legacy_row` produces a
      typed `Parsed` or returns a static reject reason; persistence
      is a separate function. Soft rejections (bad bech32, bad
      worker name, daa not parseable, hash not 64-char hex) bump
      the `rejected` counter; hard errors (connection lost) bubble
      up and abort.
    - **Resumable.** Source-side cursor is `(timestamp,
      mined_block_hash)` so a restart on row 200,000 resumes
      without rescanning the first 199,999.
    - **Dry-run mode.** Counts what would have been written without
      touching the target. Useful for pre-cutover sanity checks.
    - **JSON reconciliation report** on stdout when the binary
      completes; structured `tracing` events on stderr. The JSON
      contract is what the cutover runbook will pipe into the
      evidence collection.
  Six integration tests in
  `katpool-import-legacy/tests/import_blocks.rs` spin up a single
  postgres testcontainer with two databases on it (`legacy_test` +
  `target_test`), seed the legacy schema from
  `tests/fixtures/legacy_schema.sql`, and assert:
  insert+idempotent-skip, wallet/worker creation, matured block
  status with the right reward, deterministic correlation-id
  reproducibility, rejection of bad rows (5 distinct failure
  modes), dry-run-writes-nothing. Workspace test count: 133 → 139.
  `uuid` workspace dep gains the `v5` feature; `clap` workspace
  consumers can opt into the `env` feature individually (importer
  does).
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
