# Runbook 22 — Mainnet cutover execution

Operationalizes [`docs/cutover-plan.md`](../cutover-plan.md) Phase 10 into an
executable checklist wired to the real tooling. The cutover-plan is the
authoritative timeline + rationale; this is the "hands on keyboard" sequence.
**Do not start** until the Phase 9 gate (Runbook 21) is signed off.

## Pre-cutover gates (must all be true)

- [ ] Phase 9 acceptance recorded (Runbook 21): 4 consecutive weekly DR passes,
      chaos drills, custody EPERM suite, on-call paging both paths, load test.
- [ ] Rollback **rehearsed** on a non-prod VPS: `ops/cutover/rollback-rehearsal.sh
      <network> --execute` rolled back, came up `/ready`, and rolled forward.
- [ ] fly edge live + load-tested (`ops/edge/flyio/README.md`), origin nftables
      allowlist applied, `KATPOOL_STRATUM_PROXY_PROTOCOL=true`.
- [ ] Treasury key audit green (`katpool treasury audit`); read-only API role
      provisioned (`ops/db/api-readonly-role.sql`).
- [ ] DNS TTL lowered to 60s ≥24h ahead.

## T-72h — shadow run

- Bring up the new pool in a shadow environment subscribed to the **same**
  production bridge firehose, writing to a `katpool_shadow` schema; it does
  **not** submit blocks.
- Reconcile every 5 min: compare the shadow's per-wallet balance deltas to
  production's. Cross-schema (legacy vs new) reconciliation uses the legacy
  importer's mapping (`katpool-import-legacy` / Runbook 14). **Any divergence
  > 0 sompi pauses the cutover** and triggers investigation.
- Gate: 72h continuous, zero unexplained divergence.

## T-1h — final-state capture

- [ ] `pg_dump` production → `cutover-evidence/katpool_production_pre_cutover_<ts>.sql.gz`;
      upload to B2 with object-lock + 90d retention.
- [ ] Snapshot treasury KAS + NACHO balances; record block count + latest hash.
- [ ] Confirm DNS TTL is 60s.

## T-30m — freeze legacy

- [ ] `nft` rule on the legacy VPS refusing **new** stratum connections
      (established continue). Announce maintenance.

## T-2m — legacy stop + importer hot-run

- [ ] `docker compose stop katpool-app go-app katpool-payment katpool-monitor
      katpool-backup` (do **not** remove — kept for rollback).
- [ ] Importer hot-run (Runbook 14): set `LEGACY_DATABASE_URL` +
      `KATPOOL_DATABASE_URL`, then `./scripts/legacy-importer-rehearsal.sh --no-dry-run`.
- [ ] **Gate:** `manifest.reconcile_all_passed == "true"` **and** importer exit 0.
      Anything else → abort, roll back (below).

## T-0 — DNS flip + start

- [ ] Flip DNS to the **fly.io anycast IP** (ADR-0022): `kas.katpool.com` +
      mirrors → anycast A record (per `cutover-plan.md` T-0).
- [ ] `systemctl start katpool-<network>`; watch for "started", first stratum
      connection, first share, first block.

## T+5m … T+1h — verify, then go live

- [ ] `/ready` green; stratum accepting shares; canary credited within 5 min.
- [ ] Confirm coinbases land on the new treasury; no alerts firing.
- [ ] **T+1h:** flip payouts dry-run → live (`KATPOOL_PAYOUT_DRY_RUN=false`,
      `KATPOOL_KRC20_PAYOUT_DRY_RUN=false`); confirm the first live cycle settles.

## Rollback (any gate fails)

1. `ops/cutover/rollback-rehearsal.sh <network> --execute` (or the manual
   `cp <bak> + systemctl restart`) to restore the previous binary if the issue
   is the new pool itself.
2. Flip DNS back to the legacy edge IP.
3. `docker compose start` the stopped legacy containers (intact from T-2m).
4. The legacy importer is **append/idempotent**; re-running after a rollback is
   safe. Record the abort + cause in `cutover-evidence/`.

See `docs/cutover-plan.md` for the full rationale and the T-24h dress rehearsal.
