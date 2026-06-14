# katpool SLOs, retention, and escalation (B6)

This is the contract the alerts in `victoria-metrics/rules/` are derived from.
Thresholds in the rule files should always trace back to a line here.

## Service-level objectives

| SLO | SLI (recorded series) | Objective | Window |
|---|---|---|---|
| **API availability** | `katpool:api_availability:ratio5m` (fraction of `/ready` blackbox probes that succeed) | ≥ 99.9% | 30 days |
| **Share quality** | `katpool:share_accept_ratio:rate5m` (valid / total shares) | ≥ 95% | 7 days |
| **Block confirm rate** | `katpool:block_confirm_rate:ratio1h` (node-accepted blocks that confirm blue) | ≥ 95% | 7 days |
| **Accounting integrity** | `ks_accountant_events_lagged_total` increase + `ks_accountant_event_errors_total` rate | 0 dropped/errored events | continuous |

Alert thresholds are intentionally looser than the SLO target (e.g. invalid-share
ratio pages at >10% while the SLO is 5%) so a page means the budget is being
actively burned, not merely touched.

## Payout & treasury metrics (B7)

The KAS/KRC-20 payout engines and the consolidation engine emit these via
`katpool-metrics` (on the global registry the exporter gathers), each carrying an
`instance` label so the exporter's instance filter keeps them:

- `ks_payout_cycles_total{instance, engine, status}` — one increment per leader
  tick, by engine (`kas`/`krc20`) and terminal `PayoutCycleStatus`
  (`settled` / `partially_settled` / `broadcasting` / `planned` / `failed`, plus
  `error` for a failed tick). `PayoutCycleFailing` pages on a `failed`/`error`
  increase.
- `ks_payout_last_success_timestamp_seconds{instance, engine}` — last cycle that
  settled (fully or partially). For dashboards/stall detection; deliberately
  **not** paged on, to avoid false alarms on legitimately idle cycles (the canary
  miner is the end-to-end "are we actually paying" truth).
- `ks_treasury_balance_sompi{instance}` / `ks_treasury_spendable_utxos{instance}`
  — from the latest consolidation snapshot; `TreasuryBalanceLow` warns below an
  operator-tunable floor (see the rule). Absent if consolidation is disabled.

## Share-accept latency (B7)

The bridge emits `ks_share_accept_latency_seconds{instance}` (histogram, observed
on the accepted-share path in `bridge/src/share_handler.rs`), recorded as
`katpool:share_accept_latency:p99_5m`. Exposed for dashboards; **no alert yet** —
a latency objective has to be set here first (do not page on a guessed number).

## Known instrumentation gaps (do NOT alert on guessed metrics)

- **Canary miner** — the `CanaryMinerNotPaid` page depends on an external
  canary-miner service exporting `canary_last_credited_timestamp_seconds`, which
  does not exist yet. Until it is deployed that alert stays inert.

- `ks_payout_cycles_total{instance, engine, status}` — one increment per leader
  tick, by engine (`kas`/`krc20`) and terminal `PayoutCycleStatus`
  (`settled` / `partially_settled` / `broadcasting` / `planned` / `failed`, plus
  `error` for a failed tick). `PayoutCycleFailing` pages on a `failed`/`error`
  increase.
- `ks_payout_last_success_timestamp_seconds{instance, engine}` — last cycle that
  settled (fully or partially). For dashboards/stall detection; deliberately
  **not** paged on, to avoid false alarms on legitimately idle cycles (the canary
  miner is the end-to-end "are we actually paying" truth).
- `ks_treasury_balance_sompi{instance}` / `ks_treasury_spendable_utxos{instance}`
  — from the latest consolidation snapshot; `TreasuryBalanceLow` warns below an
  operator-tunable floor (see the rule). Absent if consolidation is disabled.

## Known instrumentation gaps (do NOT alert on guessed metrics)

- **Share-accept latency** — the bridge exposes share *counts*, not a
  submit→accept latency histogram. Needs a new histogram in
  `bridge/src/prom.rs` before a latency SLO is meaningful (next B7 follow-up).

## Retention policy

| Signal | Store | Retention | Where set |
|---|---|---|---|
| Metrics | VictoriaMetrics | 90 days | `-retentionPeriod=90d` (README) |
| Logs | Loki | 30 days | `limits_config.retention_period: 720h` |
| Traces | Tempo | 14 days | `compactor.compaction.block_retention: 336h` |

Traces are sampled and short-lived (debugging aid); metrics are the long-term
record; logs sit in between. All three fit the ~$30–40/month ADR-0004 budget.

## Escalation policy

Two severities, both routed to ntfy via Alertmanager:

- **`page`** — wake on-call now. `repeat_interval: 1h`, `group_wait: 10s`,
  ntfy `urgent` priority. Used for outages and money-path failures
  (exporter/API/stratum down, no shares, accountant errors/lag, canary unpaid).
- **`warning`** — handle next business hour. `repeat_interval: 4h`, ntfy `high`
  priority. Used for degradations (high invalid-share ratio, red-block ratio,
  abuse bursts, indexer dependency down).

A firing `page` inhibits the matching `warning` (one signal, not two). Every
alert links to a `docs/runbooks/` page; if an alert has no runbook, that is a
bug in the rule, not the runbook set.
