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

## Known instrumentation gaps (do NOT alert on guessed metrics)

The original B6 wish-list named two SLIs the pool does not emit metrics for yet.
They are tracked here rather than faked:

- **Share-accept latency** — the bridge exposes share *counts*, not a
  submit→accept latency histogram. Needs a new histogram in
  `bridge/src/prom.rs` before a latency SLO is meaningful.
- **Payout-cycle success** — the KAS/KRC-20 payout engines emit no Prometheus
  metrics. Until they do, payout health is observed via **Loki log rules** (the
  payout/treasury log lines) and the **CanaryMinerNotPaid** end-to-end probe,
  not a metric SLO. Adding payout counters (cycles started/succeeded/failed,
  treasury balance gauge) is the highest-value next instrumentation task.

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
