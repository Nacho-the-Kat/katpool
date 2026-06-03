# Runbook 20 — bump the pinned kaspa version (node + crates + toolchain)

Procedure for moving the pool to a new rusty-kaspa release: a testnet
hardfork (`tn10-toc{N}`), or the eventual mainnet cutover. The node
binary and the linked `kaspa-*` crates **must** move together — a skew
silently breaks block submission with `RuleError::BadMerkleRoot` (zero
confirmed blocks, zero rewards). Rationale:
[ADR-0017](../decisions/0017-kaspa-version-pinning.md).

## When

- kaspanet/rusty-kaspa ships a `tn10-toc{N}` you must follow on testnet.
- Mainnet activates a hardfork that supersedes the current pin.
- You are upgrading `katpool-kaspad-tn10` (see
  [Runbook 13](13-kaspad-tn10-bootstrap.md)) — do **both** sides here.

## Inputs you need first

0. **Confirm the release supports the network you deploy.** Upstream splits
   its build lines by network and the tag naming reflects it:
   - `tn10-toc{N}` (e.g. `tn10-toc3`) — **testnet-10** builds.
   - `v1.X.Y-toc.{N}` (e.g. `v1.3.0-toc.5`) — **mainnet** builds. These are
     network-gated *against* tn10: the binary exits immediately with
     `This branch does not currently support testnet-10. Please use the tn10
     branch for TN10.` Do **not** point `katpool-kaspad-tn10` at one.
   - `1.2.2-toc.4` exists only as the unreleased `toccata` branch (no tagged
     release, hence no prebuilt node binary).

   Before bumping tn10, verify a tn10-capable **release** exists
   (`git ls-remote --tags https://github.com/kaspanet/rusty-kaspa | grep tn10`)
   and that the network actually moved (a healthy toc3 node still completing
   IBD and `Accepted block … via submit block` means tn10 is still on toc3 —
   no bump needed). History: PR #67 bumped to `v1.3.0-toc.5` on the mistaken
   belief it was a tn10 upgrade; the node failed to boot and the change was
   reverted. The crate side still served as a valid *mainnet* dry run.
1. The target git **tag** (e.g. `tn10-toc4`) and its commit short SHA.
2. The upstream **`rust-version`** for that tag — read it from the
   rusty-kaspa workspace `Cargo.toml` at that tag.
3. The released node binary **SHA256** for `linux-amd64`.

## Procedure

### 1. Bump the crate pins (automated)

```bash
cd /root/katpool
./scripts/set-kaspa-version.sh tn10-toc4 <commit-short-sha>
```

This rewrites the rusty-kaspa git `tag` on every `kaspa-*`/`kaspad`
entry in the workspace `[dependencies]` and prints the coupled pins it
did **not** touch.

### 2. Bump the coupled pins (manual checklist)

Set every one of these to the upstream `rust-version` from step 0:

- `rust-toolchain.toml` → `channel`
- `Cargo.toml` `[workspace.package]` → `rust-version`
- `clippy.toml` → `msrv`
- `.github/workflows/ci.yml` → `toolchain:` on **all** jobs
- `bridge/fuzz/Cargo.toml` → `rust-version` (separate workspace)

Install the toolchain locally: `rustup toolchain install <version>`.

### 3. Bump the node binary

Edit `ops/kaspad/install-kaspad-tn10.sh`:

- `TN10_RELEASE_TAG` → new tag
- `TN10_LINUX_SHA256` → new SHA256

```bash
sudo ./ops/kaspad/install-kaspad-tn10.sh   # verifies SHA256 before install
sudo systemctl restart katpool-kaspad-tn10
```

If the on-disk DB predates the hardfork and the node fails IBD, wipe and
re-sync per [Runbook 13](13-kaspad-tn10-bootstrap.md).

### 4. Re-resolve and fix API breakage

```bash
cargo update            # re-resolves the whole graph for the new subgraph
cargo check --workspace --all-targets
```

Toccata-class hardforks change consensus structs (`UtxoEntry`,
`TransactionInput`, `TransactionOutput`, `RpcTransactionOutput`) and the
`TxScriptEngine` API. Fix construction sites in `katpool-storagemass`,
`payout-kas`, `payout-krc20`, and `accountant`. The signer/verify paths
must mirror the node's post-fork engine flags (covenants / ZK
hardening) — see `payout-kas/src/signer.rs` and `payout-krc20/src/sign.rs`.

### 5. Reconcile supply-chain policy

```bash
cargo deny check
```

Add any new git **source** (e.g. a kaspanet-forked transitive) to
`deny.toml` `[sources] allow-git`, add any new **license** to
`[licenses] allow`, and **prune** advisory ignores / license allowances
that surface as `advisory-not-detected` / `*-not-encountered` (they no
longer match the new tree). Remove any `[patch.crates-io]` entry that
`cargo` reports as unused.

### 6. Verify locally (must be green before PR)

```bash
./scripts/ci-fast.sh          # fmt + clippy + doc + machete + typos
cargo test --workspace
cargo deny check
```

### 7. Ship and confirm

Open the PR; wait for GitHub CI green. Build and redeploy per the
acceptance gate, keeping a rollback backup of the previous binary.
Confirm node ↔ crates agree on-chain:

- blocks the pool finds **confirm blue** (no `BadMerkleRoot` in the
  bridge logs),
- `share_allocation` rows accrue,
- `nacho_rebate_accrual` increments.

(See Runbooks [18](18-kas-payout-rehearsal.md) /
[19](19-krc20-payout-rehearsal.md) for the payout-side checks.)

## Rollback

Reinstate the previous `kaspad` binary backup and `systemctl restart
katpool-kaspad-tn10`; revert the PR branch. Node and crates revert
together — never leave them split.
