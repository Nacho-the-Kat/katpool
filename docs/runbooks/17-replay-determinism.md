# Runbook 17 — replay-determinism harness (Phase 3 M4)

Prove that the accountant consumer is **deterministic**: the same
`PoolEvent` stream replayed into two independent empty Postgres
databases produces byte-equal rows (modulo serial PKs and wallclock
columns).

See:

- [`accountant/src/replay.rs`](../../accountant/src/replay.rs) — snapshot + dual-verify primitives
- [`katpool-replay/`](../../katpool-replay/) — operator CLI
- [ADR-0013 § Layer 5](../decisions/0013-verification-posture.md)

## When to use this runbook

1. **Every Phase 3 PR** touching the consumer or schema — CI runs
   `accountant/tests/replay_harness_scale.rs` automatically.
2. **Pre-cutover rehearsal (T-24h)** — capture ≥ 24h of production
   events and run dual-verify + single replay (cutover evidence).
3. **Post-incident** — replay a captured NDJSON log to reproduce DB
   state in a throwaway Postgres without running the bridge.

## Event capture formats

### Canonical (preferred): NDJSON `PoolEvent`

One JSON object per line, serde shape from `katpool_domain::PoolEvent`.

Capture from the unified runtime:

```bash
export KATPOOL_EVENT_RECORD_PATH=/var/lib/katpool/events-$(date -u +%Y%m%d).ndjson
# start katpool as usual; recorder appends every bus event
```

Use this for **full lifecycle** evidence (shares, rejects, blocks).

### Legacy adapter: `katpool-app` monitoring log

The legacy stack logs share outcomes when `DEBUG=true`:

```text
20-May-2026 18:45:22 DEBUG: SharesManager 6666: Share added for rig1 - Address: kaspa:... - Nonce: 1
```

`katpool-replay --legacy-log` parses these lines. **Limitation:** block
lifecycle (`BlockFound` / `BlockAccepted`) is emitted only to Datadog
structured logs in the legacy stack, not the monitoring stream. Legacy
log replay validates share/reject ingestion determinism; block rows
require NDJSON capture or `block_details` at cutover.

## CI verification (1:50 scale)

```bash
cargo test -p accountant --test replay_harness_scale
cargo test -p accountant --test replay_determinism
cargo test -p katpool-replay
```

The scale test synthesizes ~700 events (representing ~1:50 of a busy
24h share rate) and runs dual-verify.

## Operator rehearsal

```bash
./scripts/replay-determinism-rehearsal.sh
# → replay-evidence/<UTC-stamp>-replay-determinism/manifest.json
```

Dual-verify always runs via the integration test. To replay a
captured log into your own Postgres:

```bash
cargo build --release -p katpool-replay

export KATPOOL_DATABASE_URL='postgres://...'
export KATPOOL_NETWORK='mainnet'

katpool-replay \
  --events /var/lib/katpool/events-20260527.ndjson \
  --emit-summary
```

Legacy monitoring log (subsample 1:50 for a quick smoke):

```bash
docker logs katpool-app > /tmp/katpool-monitoring.log
katpool-replay \
  --legacy-log /tmp/katpool-monitoring.log \
  --subsample-nth 50 \
  --emit-summary
```

## ≥ 24h production evidence (cutover ticket)

1. Enable `KATPOOL_EVENT_RECORD_PATH` on a shadow or staging `katpool`
   instance fed by production traffic **or** export legacy monitoring
   logs for the window (`docker logs` / log shipper archive).
2. Run dual-verify locally:
   `cargo test -p accountant --test replay_harness_scale` (synthetic gate)
   plus replay your captured NDJSON:
   `katpool-replay --events <capture.ndjson> --emit-summary`.
3. Paste the manifest + summary JSON into
   [`docs/phase-3-acceptance.md`](../phase-3-acceptance.md) § M4.

## Pass criteria

- Dual-verify test exits 0.
- `katpool-replay --emit-summary` shows non-zero `shares` (and
  `blocks` when using NDJSON capture).
- Re-running the **same** input file against a fresh DB yields the
  same snapshot content (PKs may differ).
