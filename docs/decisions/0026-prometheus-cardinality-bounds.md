---
status: accepted
date: 2026-07-17
deciders: argonmining
---

# ADR-0026: Bound Prometheus cardinality in the Stratum bridge

## Context and Problem Statement

After several weeks of uptime on mainnet, the `katpool-mainnet` bridge
process grew to ~32 GiB RSS while block-submit p95 latency climbed from
~300 ms to ~1.3 s and orphan (merged-red) block rates spiked to
15–23 %. A restart dropped memory to ~77 MiB and restored normal
submit latency.

Investigation traced the growth to **unbounded in-process Prometheus
metric cardinality** in `bridge/src/prom.rs`:

1. **`ks_mined_blocks_gauge`** — one gauge time series per block with
   unique `hash`, `nonce`, and `timestamp` labels (~44k+ series over
   weeks).
2. **`ip` label as `host:ephemeral_port`** — every miner TCP reconnect
   created a new label value on all worker-scoped counters/gauges.

The `prometheus` Rust crate retains every registered label set in
process memory. High cardinality therefore becomes a memory leak over
long uptimes and slows hot paths (share validation, block submit) as the
registry grows.

Weekly restarts would mask the symptom but violate our enterprise
reliability posture: predictable memory, bounded observability cost,
and no operational coupling between monitoring hygiene and pool
liveness.

## Decision Drivers

- **Bounded process memory** over multi-week uptimes without cron
  restarts
- **Preserve existing Grafana alerts** (`ks_blocks_mined`,
  `ks_valid_share_counter`, anti-abuse counters, etc.)
- **Keep legacy `/api/stats` recent-blocks table** for the embedded
  bridge dashboard
- **Low-cardinality Prometheus best practice** — labels identify
  dimensions, not unique events
- **Testable regression guard** — unit tests that fail if high-cardinality
  metrics are reintroduced

## Considered Options

1. **Weekly `systemctl restart` cron** — cheap operationally, does not
   fix root cause; risks orphan spikes during each restart window.
2. **Remove per-block Prometheus series; use bounded in-memory ring +
   low-cardinality counter** — drop `ks_mined_blocks_gauge`, add
   `ks_pool_blocks_found_total{instance}` and a 256-entry
   `RECENT_MINED_BLOCKS` buffer for `/api/stats`.
3. **Externalize recent blocks to Postgres only** — correct long-term
   store already exists; does not help the embedded legacy dashboard
   without extra queries on the prom HTTP server.
4. **Cap Prometheus registry with custom collector** — more invasive;
   still leaves ephemeral-port IP labels unbounded.

## Decision Outcome

**Chosen option: 2**, plus normalize the `ip` label to peer IP only
(no ephemeral port).

### Changes

| Before | After |
|---|---|
| `ks_mined_blocks_gauge{instance,worker,wallet,hash,nonce,timestamp}` | **Removed** |
| Recent blocks in `/api/stats` scraped from gauge | **256-entry in-process ring buffer** (`RECENT_MINED_BLOCKS`) |
| Block totals from gauge scrape | `ks_pool_blocks_found_total{instance}` counter |
| `ip` label = `remote_addr:remote_port` | `ip` label = `remote_addr` via `prom_peer_ip()` |

**Unchanged (alerts/dashboards keep working):**

- `ks_blocks_mined{instance,worker,miner,wallet,ip}` — per-worker block
  counter (cardinality bounded by active workers, not blocks)
- `ks_valid_share_counter`, `ks_blocks_accepted_by_node`,
  `ks_blocks_not_confirmed_blue`, anti-abuse counters, histograms

Grafana in `ops/railway/observability/` does **not** reference
`ks_mined_blocks_gauge`; removal is safe for production alerts.

### Consequences

- Positive: RSS stays in the low hundreds of MiB over multi-week
  uptimes; submit-path latency no longer degrades with registry size.
- Positive: aligns with Prometheus cardinality guidance and ADR-0004
  self-hosted observability cost model.
- Positive: unit tests guard against reintroducing `ks_mined_blocks_gauge`.
- Negative: Prometheus can no longer query historical per-block gauge
  series (they were unbounded anyway). Mitigation: blocks are persisted
  in Postgres; recent 256 remain in `/api/stats`.
- Negative: `ip` label loses per-connection disambiguation. Mitigation:
  logging and session UIDs still carry `host:port`; Prometheus `ip` is
  for miner-farm attribution, not TCP session identity.
- Negative: existing worker time series keyed by `host:port` will age out
  after deploy; new series use IP-only. One-time metric discontinuity,
  not a functional regression.

### Confirmation

1. **Unit tests** in `bridge/src/prom.rs`:
   - `ks_mined_blocks_gauge` not registered
   - `RECENT_MINED_BLOCKS` capped at 256
   - `prom_peer_ip` omits ephemeral port
2. **Post-deploy (24–72 h)** on mainnet:
   - `ps` RSS stable (not climbing toward GiB)
   - `curl :9303/metrics | rg '^ks_' | wc -l` grows slowly with workers,
     not with blocks or reconnects
   - Block submit latency p95 < ~200 ms
   - Orphan rate trends back toward baseline (<1 %)
3. **Optional ops hardening** (separate change): `MemoryMax` systemd
   drop-in + alert on bridge RSS — defense in depth, not a substitute
   for this fix.

## Pros and Cons of the Options

### Option 1: Weekly restart

- Good: zero code change
- Bad: treats symptom; recurring orphan/latency risk; not enterprise-grade

### Option 2: Bounded ring + low-cardinality counter (chosen)

- Good: fixes root cause; preserves alerts; testable
- Bad: loses per-block Prometheus gauge history (acceptable)

### Option 3: Postgres-only recent blocks

- Good: single source of truth
- Bad: extra latency/complexity on prom HTTP path; out of scope for hot fix

### Option 4: Custom capped collector

- Good: generic
- Bad: heavy engineering; does not fix IP port cardinality alone

## More Information

- Related: [ADR-0004](0004-self-host-observability.md) (metrics stack)
- Implementation: `bridge/src/prom.rs`, `bridge/src/share_handler.rs`
- Incident evidence: Jul 2026 mainnet memory/orphan/latency correlation
  (conversation transcript `5f4df347-39e7-46d6-9229-c8059d9fd770`)
