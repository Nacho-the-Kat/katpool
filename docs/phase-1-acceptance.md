# Phase 1 acceptance evidence

Phase 1 closes when every row below is **GREEN** for a release-candidate
commit. The Phase 2 (database schema) work-stream cannot start until
this page is complete.

## Acceptance matrix

| # | Criterion | Verification | Status |
|---|---|---|---|
| 1 | Vendored bridge is byte-for-byte verifiable against upstream `rusty-kaspa v1.1.0`, with every local divergence documented. | `diff -r --exclude=Cargo.toml --exclude=.gitignore --exclude=.gitattributes /tmp/verify-bridge/bridge bridge` shows only the rows in `bridge/UPSTREAM.md` | GREEN — landed in PR #1 |
| 2 | `katpool-domain` types validate every primitive at construction, with typed errors and transparent serde. | `cargo test -p katpool-domain` — 42 tests | GREEN — landed in PR #2 |
| 3 | Stratum bridge emits one `PoolEvent` per submission outcome and per block lifecycle event. | `cargo test -p kaspa-stratum-bridge event_bus` — 6 tests; bus-tap during the testnet-10 smoke | GREEN (unit), pending live tap | PR #2 |
| 4 | Per-IP connection cap is enforced. | `cargo test -p kaspa-stratum-bridge anti_abuse` — 18 tests, including `conn_cap_per_ip_blocks_after_threshold`. | GREEN — landed in PR #3 |
| 5 | Per-IP frame-rate token bucket is enforced. | Same test suite, `token_bucket_allows_burst_then_throttles` + `token_bucket_refill_caps_at_burst`. | GREEN — landed in PR #3 |
| 6 | Address-parse-or-disconnect on `mining.authorize` failure. | Code path in `bridge/src/default_client.rs`, `clean_wallet` → `ctx.disconnect()`. Manual e2e during testnet-10 smoke. | GREEN (code), pending live exercise | PR #3 |
| 7 | Malformed-frame Prometheus counter wired and exported. | `bridge/src/prom.rs` registers `ks_anti_abuse_malformed_frame_total`; testnet smoke records a non-zero value when fed garbage. | GREEN (code), pending live exercise | PR #3 |
| 8 | Stratum parser fuzz harness ≥ 1M iterations with zero panics. | `cd bridge/fuzz && cargo +nightly fuzz run stratum_parser -- -runs=1500000 -max_total_time=60`. Result recorded in `bridge/fuzz/README.md`. | GREEN — 1,500,000 iterations, 23 s, 0 panics (2026-05-25) | PR #3 |
| 9 | systemd unit reaches `systemd-analyze security` score ≤ 2.5. | `systemd-analyze security ops/systemd/katpool-bridge.service` (offline). | GREEN — exposure level 1.1 OK (2026-05-25) | PR #4 |
| 10 | Anti-abuse limits are operator-tunable via env (no recompile). | `cargo test -p kaspa-stratum-bridge anti_abuse::tests::from_lookup_*` (5 deterministic tests). | GREEN — landed in PR #4 |
| 11 | Bridge cold-boots in `< 5 s` in external mode against a live kaspad-testnet-10. | `scripts/testnet10-smoke.sh` JSON, `boot.ok == true`. | **PENDING live run** — run before next mainnet release |
| 12 | Bridge accepts ≥ 100 valid shares from a CPU miner over a 60 s window. | Same script, `shares.ok == true`. | **PENDING live run** |
| 13 | Bridge mines ≥ 1 block in that 60 s window. | Same script, `blocks.ok == true`. | **PENDING live run** |
| 14 | `cargo deny check` is clean against the locked Cargo.lock. | CI step; locally verifiable with `cargo deny check`. | GREEN — every Phase 1 PR |

## Run history

| Date (UTC) | Commit | Boot time | Shares (60 s) | Blocks (60 s) | Run by | Notes |
|---|---|---|---|---|---|---|
| _pending_ | _pending_ | — | — | — | _operator_ | First live smoke against testnet-10. Required before mainnet release. |

Append a row every time you re-run the smoke. Negative results (missed
acceptance) require an issue + PR; positive results unblock the next
release candidate.

## How to re-run

```bash
# On a host with a kaspad-testnet-10 reachable at 127.0.0.1:16210
export KATPOOL_TESTNET10_WALLET=kaspatest:qrxd24c5w6pl2qa9k7q5e0lyepuu4r5t2f6awvxllk0a83qqfys9
scripts/testnet10-smoke.sh | tee phase-1-acceptance-$(date -u +%FT%TZ).json
```

Detailed instructions and tear-down: `docs/runbooks/12-testnet10-smoke.md`.

## Cross-references

- `bridge/UPSTREAM.md` — vendored divergence register
- `bridge/fuzz/README.md` — fuzz reproducibility
- `ops/systemd/katpool-bridge.service` — hardened deployment unit
- `docs/runbooks/12-testnet10-smoke.md` — operator-facing smoke runbook
- `docs/decisions/0002-fork-rusty-kaspa-bridge.md` — vendoring rationale
