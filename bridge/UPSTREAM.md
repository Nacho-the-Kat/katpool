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

Only `bridge/` is in our git history. The 11 internal `kaspa-*` crates
the bridge depends on (`kaspa-consensus-core`, `kaspa-grpc-client`,
`kaspa-pow`, `kaspa-hashes`, `kaspa-rpc-core`, `kaspa-rpc-service`,
`kaspa-addresses`, `kaspa-notify`, `kaspa-core`, `kaspa-utils`,
`kaspad`) are pulled as **cargo git dependencies** pinned to the same
`v1.1.0` tag, declared in the workspace root [`Cargo.toml`](../Cargo.toml)
under `[workspace.dependencies]`.

Why not crates.io: as of 2026-05-25 none of the 11 kaspa-* crates are
published to crates.io. The kaspa core team's release model is the
rusty-kaspa monorepo tag.

This means our `deny.toml` `[sources]` table includes
`https://github.com/kaspanet/rusty-kaspa` under `allow-git`. Any other
git dependency requires a fresh ADR + PR.

## Local divergence from upstream

Track every intrusive patch here. Anything not listed is a verbatim
copy of upstream `bridge/` at v1.1.0.

| File | Upstream | Our change | Phase |
|---|---|---|---|
| `Cargo.toml` | inherits rusty-kaspa workspace metadata (`include.workspace = true`, `[lints] workspace = true`) | (1) Drop `include.workspace = true` because our workspace.package has no `include` field (we publish nothing). (2) Replace `[lints] workspace = true` with an explicit minimal set matching rusty-kaspa's own workspace lints (`empty_docs = allow`, `uninlined_format_args = allow`, `[lints.rust] warnings = allow`, `[lints.rustdoc] all = allow`). Our strict pedantic-and-nursery lints + `-D warnings` in CI would generate ~963 errors on upstream code that we don't want to touch. Our own crates remain on the strict workspace defaults. | 1 (vendoring) |
| `rustfmt.toml` (new file) | upstream has `/.rustfmt.toml` at the rusty-kaspa workspace root with `max_width = 135` + `use_small_heuristics = "Max"`. | Copied that config into `bridge/rustfmt.toml` so rustfmt formats this crate to upstream's style. Our workspace `/rustfmt.toml` uses `max_width = 100` for our own code. Without the per-directory override, `cargo fmt --check` would require a 1000+ line reformat of upstream. | 1 (vendoring) |
| `src/share_handler.rs` | — | _Phase 1, planned_: emit `PoolEvent::{ShareCredited, ShareRejected, BlockFound, BlockAccepted}` on a `tokio::sync::broadcast` channel injected at construction | 1 (event bus) |
| `src/stratum_listener.rs` | — | _Phase 1, planned_: per-IP connection cap, per-IP share-rate token bucket, address-parse-or-disconnect, malformed-frame metric | 1 (anti-abuse) |
| _Anything else added later_ | | | |

## Workspace integration side-effects (not in `bridge/` itself)

These changes live in the workspace root but are caused by vendoring
the bridge. They are tracked here so future re-vendor sees the full
picture.

### `/Cargo.toml`

- Added 11 `kaspa-*` crates and `kaspad` as `[workspace.dependencies]`
  with `git = "https://github.com/kaspanet/rusty-kaspa", tag = "v1.1.0"`.
  These are not on crates.io as of 2026-05-25; the rusty-kaspa monorepo
  tag is the kaspa core team's release vehicle.
- Added 8 transitive non-kaspa workspace deps the bridge needs
  (`dirs`, `num-traits`, `once_cell`, `futures-util`, `parking_lot`,
  `regex`, `uuid`, `clap`).
- Added a `[patch.crates-io]` entry routing `serde_nested_with` to its
  GitHub source at tag `0.2.6`. Upstream crates.io has yanked every
  published version of `serde_nested_with`, but `kaspa-rpc-core` still
  requires it transitively. The GitHub source compiles cleanly.

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
