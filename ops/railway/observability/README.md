# katpool observability stack (Railway LGTM) — provisioning guide

Self-hosted Grafana **LGTM** stack per
[ADR-0004](../../../docs/decisions/0004-self-host-observability.md): replace
Datadog/Telegram with Grafana + Loki + Tempo + VictoriaMetrics + Alertmanager +
Blackbox + GlitchTip + Uptime Kuma + ntfy, on Railway, in a **separate failure
domain** from the pool (a pool-VPS outage must not blind the monitoring).

## What is in this repo vs. what you do in Railway

This directory is the **config-as-code** for the stack: the files each service
mounts and consumes. **Standing up the Railway project** — creating services,
attaching volumes, setting image digests, and storing secrets — is operator
action (it needs the Railway account; see the checklist below).

> There is intentionally **no single `railway.toml`** here. Railway config-as-code
> is *per-service* (each service sets its own Root Directory + Config File, e.g.
> `katpool-landing/railway.toml`), so a multi-service project cannot be defined
> by one file. **This README is the project definition** the ADR-0004
> confirmation refers to; the per-service configs live in the subfolders below.

## Services

Internal traffic uses Railway private DNS (`<service>.railway.internal`) and is
free. Only Grafana, the ntfy server, and the Uptime Kuma status page need public
domains. **Pin each image to a digest** at provisioning (resolve the current
stable tag, then pin `@sha256:…` — same discipline as the CI actions).

| Service | Image (pin digest) | Internal port | Volume | Config from this repo |
|---|---|---|---|---|
| VictoriaMetrics | `victoriametrics/victoria-metrics` | 8428 | `/victoria-metrics-data` | `victoria-metrics/scrape.yml` |
| vmalert | `victoriametrics/vmalert` | 8880 | — | `victoria-metrics/rules/*.yml` |
| Grafana | `grafana/grafana` | 3000 | `/var/lib/grafana` | `grafana/provisioning/**`, `grafana/dashboards/**` |
| Loki | `grafana/loki` | 3100 | `/loki` | `loki/loki-config.yaml` |
| Tempo | `grafana/tempo` | 3200 / 4317 | `/var/tempo` | `tempo/tempo.yaml` |
| Alertmanager | `prom/alertmanager` | 9093 | `/alertmanager` | `alertmanager/alertmanager.yml` |
| Blackbox exporter | `prom/blackbox-exporter` | 9115 | — | `blackbox/blackbox.yml` |
| ntfy | `binwiederhier/ntfy` | 80 | `/var/cache/ntfy` | (operator; holds token) |
| ntfy-alertmanager | `ghcr.io/xenrox/ntfy-alertmanager` | 8080 | — | (operator; holds token) |
| Uptime Kuma | `louislam/uptime-kuma:1` | 3001 | `/app/data` | (operator; UI-configured) |
| GlitchTip | `glitchtip/glitchtip` (+ Postgres + Redis) | 8000 | DB volume | (operator; needs DB+broker) |
| **canary-miner** | *(deferred — see below)* | — | — | — |

## Metrics flow (B4)

The pool's `/metrics` binds **loopback** on mainnet
(`KATPOOL_PROM_PORT=127.0.0.1:9302`) and is instance-filtered, so it cannot be
scraped across the network. Therefore:

- **Origin → VM (pull-local, push-remote):** run **vmagent on the pool VPS** with
  `victoria-metrics/origin-vmagent.yml`; it scrapes `127.0.0.1:9302` and
  `-remoteWrite.url`s into VictoriaMetrics on Railway (basic-auth from the host
  EnvironmentFile).
- **Railway-side scrape:** VictoriaMetrics runs `scrape.yml` for its own metrics
  and the **Blackbox** synthetic probes of the pool's *public* surface
  (`/health` `/ready` `/started`, stratum TCP, indexer health).

Run VictoriaMetrics with `-promscrape.config=/etc/vm/scrape.yml
-retentionPeriod=90d`. Set the `%{KATPOOL_API_HOST}` / `%{KATPOOL_STRATUM_HOST}`
env placeholders on the VM service.

Run vmalert with:

```
vmalert \
  -rule=/etc/vmalert/rules \
  -datasource.url=http://victoriametrics.railway.internal:8428 \
  -remoteWrite.url=http://victoriametrics.railway.internal:8428 \
  -remoteRead.url=http://victoriametrics.railway.internal:8428 \
  -notifier.url=http://alertmanager.railway.internal:9093 \
  -evaluationInterval=30s
```

## Logs (B4)

The unified runtime emits structured JSON when `KATPOOL_LOG_FORMAT=json`
(katpool-telemetry). Ship those journald logs from the origin to Loki with a
shipper (Promtail/Grafana Alloy/vector — operator choice) targeting
`http://loki.railway.internal:3100`. Loki retains 30 days and can run log-based
alert rules (for payout/treasury lines that have no metric yet).

## Traces (B4)

Set `KATPOOL_OTLP_ENDPOINT` on the pool to the Tempo distributor
(`http://tempo.railway.internal:4317`, OTLP/gRPC). Off by default until the
stack exists.

## Paging (ntfy) (B5)

Alertmanager → **ntfy-alertmanager bridge** → ntfy. The bridge
(<https://git.xenrox.net/~xenrox/ntfy-alertmanager>) maps the alert `severity`
label to ntfy priority/topic and holds the **ntfy token** — so its config is a
**secret**, kept in Railway service variables, not in this repo. Alertmanager
posts to the bridge with a webhook password read from a mounted file
(`/etc/alertmanager/secrets/webhook_password`); set the same credentials on the
bridge. See `alertmanager/alertmanager.yml`.

## Status page

Uptime Kuma provides the public status page and an independent (second-source)
uptime check; point it at the same public `/health` endpoint and add an ntfy
notification. GlitchTip (Sentry-compatible) receives application error events.

## Canary miner (deferred)

ADR-0004 calls for a small external miner submitting **real shares from outside
the VPS**, exporting `canary_last_credited_timestamp_seconds`; the
`CanaryMinerNotPaid` page (already in `katpool-alerts.yml`) fires when that
end-to-end accept→credit path breaks. The canary **binary** is a separate
deliverable — the alert is in place and inert until the metric exists.

## Provisioning checklist

1. Create the Railway project; add each service with its pinned image + volume.
2. Mount this repo's config files into each service (Config File path / volume).
3. Set env: `KATPOOL_API_HOST`, `KATPOOL_STRATUM_HOST` (VM); ntfy token + topics
   (ntfy, bridge); webhook password (Alertmanager + bridge); Grafana admin.
4. Install vmagent on the pool VPS with `origin-vmagent.yml` + remote_write creds.
5. Set `KATPOOL_LOG_FORMAT=json`, deploy a log shipper to Loki, set
   `KATPOOL_OTLP_ENDPOINT` for traces.
6. Verify: Grafana shows the **katpool — Pool Overview** dashboard with live
   data; stop the canary (once built) and confirm `CanaryMinerNotPaid` fires.
