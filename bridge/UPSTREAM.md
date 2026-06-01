# bridge/ — upstream provenance

This directory is a one-time verbatim copy of the `bridge/` subdirectory
of [`kaspanet/rusty-kaspa`](https://github.com/kaspanet/rusty-kaspa) at
release **v1.1.0** (commit
[`e97070faa3826c590f477e327c82daaddd6178f4`](https://github.com/kaspanet/rusty-kaspa/commit/e97070faa3826c590f477e327c82daaddd6178f4),
published 2026-03-04).

The decision to fork rather than depend on or submodule is captured in
[ADR-0002](../docs/decisions/0002-fork-rusty-kaspa-bridge.md). The
operational note here is just the provenance and the local divergence
register.

## Vendoring mechanism

`git subtree add` was the original plan, but `git subtree` grafts the
**entire** upstream history under the `bridge/` prefix — there is no
way for `git subtree` to pull only a subdirectory of the source repo
(it requires a pre-filtered repo upstream). Pulling all of rusty-kaspa
into our `bridge/` would bloat the repo with the full node, wallet,
consensus, and protocol code we are not modifying.

Instead, the bridge source was copied as a one-time snapshot:

```bash
git clone --depth 1 --branch v1.1.0 --filter=blob:none --sparse \
  https://github.com/kaspanet/rusty-kaspa.git /tmp/rusty-kaspa-v1.1.0
cd /tmp/rusty-kaspa-v1.1.0
git sparse-checkout set bridge
cp -r bridge /root/katpool/bridge
rm /root/katpool/bridge/.gitignore /root/katpool/bridge/.gitattributes
```

The deleted `.gitignore` and `.gitattributes` were rusty-kaspa-specific
(they referenced paths outside the bridge directory) and are replaced
by our repo-root equivalents.

## Dependency model

Only `bridge/` is in our git history. The internal `kaspa-*` crates
the bridge depends on (`kaspa-consensus-core`, `kaspa-grpc-client`,
`kaspa-pow`, `kaspa-hashes`, `kaspa-rpc-core`, `kaspa-rpc-service`,
`kaspa-addresses`, `kaspa-notify`, `kaspa-core`, `kaspa-utils`,
`kaspad`) are pulled as **cargo git dependencies** declared in the
workspace root [`Cargo.toml`](../Cargo.toml) under
`[workspace.dependencies]`.

Why not crates.io: none of the kaspa-* crates are published to
crates.io. The kaspa core team's release model is the rusty-kaspa
monorepo tag.

> **Dependency tag ≠ source snapshot.** The vendored `bridge/` source in
> this directory is the **`v1.1.0`** snapshot (see top of file), but the
> `kaspa-*`/`kaspad` dependency tag has since advanced to **`tn10-toc3`**
> (`1.2.1-toc.3`, commit `1015a62`) to match the testnet-10 Toccata
> hardfork the live node runs. The v1.1.0 bridge source compiles and
> submits blocks correctly against the toc3 crates because transaction
> serialization, hashing, and SMT/merkle computation live in the crates,
> not the bridge source; pinning the crates at v1.1.0 against a toc3 node
> caused `RuleError::BadMerkleRoot` on every found block. Bumping the
> dependency tag is governed by
> [ADR-0017](../docs/decisions/0017-kaspa-version-pinning.md) and
> [Runbook 20](../docs/runbooks/20-kaspa-version-bump.md); re-vendoring
> the bridge source from a newer tag remains future work.

This means our `deny.toml` `[sources] allow-git` table includes
`https://github.com/kaspanet/rusty-kaspa` and
`https://github.com/kaspanet/workflow-perf-monitor-rs` (a transitive the
toc3 tree pins to a git tag). Any other git dependency requires a fresh
ADR + PR.

## Local divergence from upstream

Track every intrusive patch here. Anything not listed is a verbatim
copy of upstream `bridge/` at v1.1.0.

| File | Upstream | Our change | Phase |
|---|---|---|---|
| `Cargo.toml` | inherits rusty-kaspa workspace metadata (`include.workspace = true`, `[lints] workspace = true`) | (1) Drop `include.workspace = true` because our workspace.package has no `include` field (we publish nothing). (2) Replace `[lints] workspace = true` with an explicit minimal set matching rusty-kaspa's own workspace lints (`empty_docs = allow`, `uninlined_format_args = allow`, `[lints.rust] warnings = allow`, `[lints.rustdoc] all = allow`). Our strict pedantic-and-nursery lints + `-D warnings` in CI would generate ~963 errors on upstream code that we don't want to touch. Our own crates remain on the strict workspace defaults. | 1 (vendoring) |
| `rustfmt.toml` (new file) | upstream has `/.rustfmt.toml` at the rusty-kaspa workspace root with `max_width = 135` + `use_small_heuristics = "Max"`. | Copied that config into `bridge/rustfmt.toml` so rustfmt formats this crate to upstream's style. Our workspace `/rustfmt.toml` uses `max_width = 100` for our own code. Without the per-directory override, `cargo fmt --check` would require a 1000+ line reformat of upstream. | 1 (vendoring) |
| `src/share_handler.rs` | — | **Phase 1, milestone 2 — landed**: emit `PoolEvent::{ShareCredited, ShareRejected, BlockFound, BlockAccepted}` on a `tokio::sync::broadcast` channel injected via `ShareHandler::with_event_bus`. Adds an `Option<broadcast::Sender<PoolEvent>>` field, a single `emit` helper, a shared `correlation_id` generated at the top of `handle_submit`, a `build_share_rejected` helper, and a `mod event_bus_tests` test module at end-of-file. Best-effort emission: drops events silently when no bus is attached or all receivers have been dropped — never stalls share processing. | 1 (event bus) |
| `src/anti_abuse.rs` (new file) | — | **Phase 1, milestone 3 — landed**: per-IP `AntiAbuseGuard` with connection cap, tracked-IP cap, and a token-bucket frame-rate limiter. Time-injected for deterministic unit testing; RAII `ConnTicket` releases per-IP slots on connection drop. **Phase 1, milestone 4 — landed**: added `AntiAbuseConfig::from_lookup` (pure, closure-injected) and `AntiAbuseConfig::from_env` (production wrapper) with five additional deterministic unit tests, plus `AntiAbuseConfigError::InvalidEnvValue` variant. | 1 (anti-abuse) |
| `src/stratum_listener.rs` | — | **Phase 1, milestone 3 — landed**: anti-abuse hook at TCP-accept (reject with `record_anti_abuse_connection_reject`), per-frame token-bucket gate before handler dispatch (`record_anti_abuse_frame_limited` + disconnect), JSON-RPC parse-error counter (`record_malformed_frame`). `StratumListenerConfig` grows `anti_abuse: Arc<AntiAbuseGuard>` and `instance_id: String`; `spawn_client_listener` gains matching parameters and an `IpAddr` parse from `ctx.remote_addr` at task entry. | 1 (anti-abuse) |
| `src/default_client.rs` | — | **Phase 1, milestone 3 — landed**: `handle_authorize` now `ctx.disconnect()`s and increments `ks_anti_abuse_bad_address_total` when `clean_wallet` rejects the bech32 address, instead of merely propagating the error. | 1 (anti-abuse) |
| `src/client_handler.rs` | — | **Phase 1, milestone 3 — landed**: added `pub fn instance_id(&self) -> &str` getter so the anti-abuse hook in `default_client.rs` can label its Prometheus counters. | 1 (anti-abuse) |
| `src/prom.rs` | — | **Phase 1, milestone 3 — landed**: four new `CounterVec`s — `ks_anti_abuse_connection_reject_total{reason}`, `ks_anti_abuse_frame_rate_limited_total`, `ks_anti_abuse_malformed_frame_total`, `ks_anti_abuse_bad_address_total` — plus matching `record_*` helpers. Registered in `init_metrics()`. | 1 (anti-abuse) |
| `src/stratum_server.rs` | — | **Phase 1, milestone 3 — landed**: constructs the per-process `AntiAbuseGuard` with `AntiAbuseConfig::production()` defaults and threads it (plus `instance_id`) into `StratumListenerConfig`. **Phase 1, milestone 4 — landed**: switched to `AntiAbuseConfig::from_env()` so operators tune limits via `KATPOOL_ANTI_ABUSE_*` env vars (see `ops/systemd/katpool-bridge.conf.d/anti-abuse.conf.example`). Malformed values fail-fast at start-up with an explanatory `io::Error`. **Phase 3, M3d — landed**: new public function `listen_and_serve_with_events` that accepts an optional `broadcast::Sender<PoolEvent>` and wires it into `ShareHandler::with_event_bus`. The original `listen_and_serve` is preserved as a thin wrapper around the new function passing `None`, so the bridge's own `main.rs` keeps the upstream call shape; the unified `katpool` runtime binary uses the new variant to embed bridge + accountant in one process. | 1 (anti-abuse, env tuning), 3 (event bus → accountant) |
| `src/kaspaapi.rs` | — | **Phase 3, M3e — landed**: `KaspaApi::new` accepts an additional `coinbase_address_override: Option<Address>`. When `Some`, every `get_block_template` call replaces the miner-supplied `wallet_addr` with the configured pool address before calling kaspad — this is custodial PROP-pool mode: every block's coinbase pays the pool, which the accountant then pro-rates across miners by share weight. When `None`, preserves upstream solo / MM-pool behaviour (each miner mines to themselves). The bridge's own `main.rs` passes `None`; only the unified `katpool` runtime opts in via `KATPOOL_POOL_ADDRESS`. Logic factored into a pure `resolve_coinbase_recipient` helper with 4 dedicated unit tests. | 3 (custodial PROP pool) |
| `src/main.rs` | — | **Phase 3, M3e — landed**: one-line constructor-arg addition `None` matching the new `KaspaApi::new` signature. Preserves upstream solo / MM-pool semantics for the standalone bridge binary. | 3 (custodial PROP pool) |
| `src/lib.rs` | — | **Phase 1, milestone 3 — landed**: `pub mod anti_abuse;`. | 1 (anti-abuse) |
| `fuzz/` (new subdirectory, non-workspace) | — | **Phase 1, milestone 3 — landed**: cargo-fuzz harness for `jsonrpc_event::unmarshal_event`. Standalone crate (excluded from workspace because libfuzzer-sys is nightly-only); checked-in `Cargo.lock` mirrors the parent's to keep the rusty-kaspa subgraph pinned. See `bridge/fuzz/README.md` for build/run/acceptance instructions. | 1 (fuzz) |
| _Anything else added later_ | | | |

## Workspace integration side-effects (not in `bridge/` itself)

These changes live in the workspace root but are caused by vendoring
the bridge. They are tracked here so future re-vendor sees the full
picture.

### `/Cargo.toml`

- Added the `kaspa-*` crates and `kaspad` as `[workspace.dependencies]`
  with `git = "https://github.com/kaspanet/rusty-kaspa", tag = "..."`.
  These are not on crates.io; the rusty-kaspa monorepo tag is the kaspa
  core team's release vehicle. The tag is bumped via
  `scripts/set-kaspa-version.sh` (currently `tn10-toc3`; see ADR-0017).
- Added 8 transitive non-kaspa workspace deps the bridge needs
  (`dirs`, `num-traits`, `once_cell`, `futures-util`, `parking_lot`,
  `regex`, `uuid`, `clap`).
- The `[patch.crates-io]` entry routing `serde_nested_with` to its
  GitHub source (needed under v1.1.0) was **removed** at the `tn10-toc3`
  bump: the toc3 `kaspa-rpc-core` no longer depends on it, and `cargo`
  flagged the patch as unused.

### `/Cargo.lock`

- Adopted rusty-kaspa v1.1.0's `Cargo.lock` wholesale as the starting
  lockfile, then `cargo update`d for our own crates. Reason: rusty-kaspa
  carefully pinned ~200 transitive deps at versions that compile
  together; a fresh resolve picks newer versions of `wasm-bindgen`,
  `chrono`, `js-sys`, etc., that break `workflow-core` 0.18.0's
  WASM-bindgen API expectations. Adopting the upstream lock avoids this
  cascade. Two yanked entries (`crossbeam-channel`, `serde_nested_with`)
  were re-resolved post-adopt to non-yanked versions to keep
  `cargo deny check advisories` clean.

### `/deny.toml`

- `[sources] allow-git` now permits `kaspanet/rusty-kaspa` and
  `murar8/serde_nested_with`.
- `[advisories] ignore` lists 16 RustSec advisories triggered by the
  rusty-kaspa pinned subgraph, each with a per-advisory rationale.
  Re-evaluate the entire block on each re-vendor.
- `[licenses] allow` adds `MPL-2.0`, `CC0-1.0`, `Unicode-DFS-2016`.
- `[licenses] exceptions` allows `LGPL-3.0-only` for the three
  `malachite-*` crates (pulled by `kaspa-math`); `OpenSSL`/`ISC`/`MIT`
  for `ring`; `MIT` for `workflow-perf-monitor`.
- `[[licenses.clarify]]` entries pin `ring`'s composite-license file
  and `workflow-perf-monitor`'s low-confidence MIT detection.
- `[bans] skip-tree` adds `kaspa-stratum-bridge` and `kaspad` so that
  upstream's transitive duplicate versions (intentional pinning for
  compatibility) don't trip the `multiple-versions = "deny"` rule.

When upstream releases a new bridge version, re-vendor by running the
snapshot commands again over the new commit, then re-apply each row
above as a fresh patch. The intrusive surface is intentionally narrow
to minimise this merge cost.

## How to verify the snapshot matches upstream

```bash
# From a fresh clone of katpool:
git clone --depth 1 --branch v1.1.0 --filter=blob:none --sparse \
  https://github.com/kaspanet/rusty-kaspa.git /tmp/verify-bridge
git -C /tmp/verify-bridge sparse-checkout set bridge

# Compare only the source tree (ignoring Cargo.toml, which is the only
# intentional difference at vendor time):
diff -r \
  --exclude=Cargo.toml \
  --exclude=.gitignore \
  --exclude=.gitattributes \
  /tmp/verify-bridge/bridge \
  bridge
# Expect: empty output (no diff) immediately after vendoring.
# After Phase 1 patches land: only the files listed in the divergence
# table above should appear, with the listed changes.
```
