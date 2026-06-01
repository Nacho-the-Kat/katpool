# Runbook 19 — KRC-20 NACHO payout dry-run rehearsal

A one-shot binary (`katpool-krc20-rehearsal`) that drives **one dry-run
NACHO payout cycle** through the production engine
(`payout_krc20::Krc20PayoutEngine` in `ExecutionMode::DryRun`): it acquires
the single-leader advisory lock, derives the DAA cycle window, quotes the
NACHO floor price (`api.kaspa.com`, fail-closed behind a circuit breaker),
plans the eligible rebates into commit/reveal transfers, and for every
pending transfer **mass-plans + signs + verifies the commit against the
live treasury UTXO set** — **without recording a txid, broadcasting, or
crediting any `nacho_rebate`**. The planned cycle, the planned
`krc20_pending_transfer` rows, and the `krc20_cycle.plan` /
`krc20_cycle.reconcile` audit trail (the `plan` entry carries the quoted
floor price) are the Phase 5 sign-off evidence.

See:

- [Phase 5 acceptance](../phase-5-acceptance.md) — row 6 (this rehearsal)
- [`katpool-krc20-rehearsal/src/`](../../katpool-krc20-rehearsal/src) — the tool
- [Runbook 18 — KAS payout dry-run rehearsal](18-kas-payout-rehearsal.md) — the Phase 4 KAS analogue this mirrors
- [ADR-0015](../decisions/0015-krc20-inscription-envelope.md) — the kasplex inscription envelope being planned
- [ADR-0016](../decisions/0016-krc20-payout-conversion-and-floor-price.md) — the KAS→NACHO conversion and floor-price quote
- [Runbook 11 — Treasury key rotation](11-key-rotation.md) — how the treasury key is delivered in production
- [Runbook 13 — kaspad-tn10 bootstrap](13-kaspad-tn10-bootstrap.md) — the testnet-10 node this runs against

## When to use this runbook

Two triggers:

1. **Phase 5 acceptance sign-off.** Run once against testnet-10 with a
   funded treasury and at least one wallet carrying a pending NACHO rebate,
   to produce the reconcile JSON + audit log + manifest that close
   acceptance row 6. Archive under `payout-evidence/`.
2. **Pre-enable smoke check.** Before flipping
   `KATPOOL_KRC20_PAYOUT_ENABLED=true` (and later
   `KATPOOL_KRC20_PAYOUT_DRY_RUN=false`) on any environment, run the
   rehearsal to confirm the floor-price quote succeeds, the treasury funds
   every commit, and each commit mass-plans + signs cleanly.

This tool **never** broadcasts and **never** mutates `nacho_rebate.paid_sompi`.
Real payouts run inside the `katpool` runtime via the engine; see
[Phase 5 acceptance](../phase-5-acceptance.md).

## Preconditions

- A target Postgres with the new katpool schema migrated and at least one
  wallet with a pending NACHO rebate (`nacho_rebate_accrual.accrued >
  paid`); otherwise the cycle plans zero transfers — still a valid, if
  empty, rehearsal.
- A reachable testnet-10 kaspad gRPC endpoint (see Runbook 13).
- Outbound HTTPS to the floor-price API (`KATPOOL_KRC20_QUOTE_BASE`,
  default `api.kaspa.com`). The quote **fails the tick closed** if the
  source is unreachable or degraded — a deliberate safety property, not a
  bug. A hard exit (non-0/2/3) with a quote error in `reconcile.log` means
  fix connectivity and re-run.
- A funded treasury address on testnet-10 (the first `KATPOOL_POOL_ADDRESS`),
  and the matching raw 32-byte **hex** key in a file readable only by the
  operator. For testnet rehearsal the key is a hex file, not a systemd
  credential — production delivery is Runbook 11.
- No other `katpool` instance holding the KRC-20 payout leader lock on the
  same DB (the rehearsal shares the `payout-krc20:nacho-leader` advisory
  key; a live leader makes the rehearsal exit `3` without doing anything).
- `katpool-krc20-rehearsal` built (`cargo build --release -p katpool-krc20-rehearsal`).
- `jq` + `sha256sum` available for the wrapper script.

## Command

Use the wrapper script — it captures the JSON envelope, the tracing log,
the cycle audit trail, and a manifest (git rev + binary sha256 + exit code)
into a timestamped artefact directory under `payout-evidence/`.

```bash
export KASPAD_GRPC_URL='grpc://127.0.0.1:16210'
export KATPOOL_DATABASE_URL='postgres://katpool_rw@db/katpool'
export KATPOOL_POOL_ADDRESS='kaspatest:qr...treasury'
export KATPOOL_TREASURY_KEY_PATH='/run/secrets/treasury-key.hex'  # raw 32-byte hex
# Optional: KATPOOL_KRC20_TICKER, KATPOOL_KRC20_QUOTE_BASE,
#           KATPOOL_KRC20_MIN_PENDING_SOMPI, KATPOOL_KRC20_MIN_NACHO_BASE_UNITS,
#           KATPOOL_KRC20_PAYOUT_CYCLE_SPAN_DAA, KATPOOL_NETWORK
./scripts/krc20-payout-rehearsal.sh
# → writes to ./payout-evidence/<UTC-stamp>-krc20-dry-run/
```

Lower-level invocation (CI / no `jq`):

```bash
katpool-krc20-rehearsal \
  --kaspad-url "$KASPAD_GRPC_URL" \
  --database-url "$KATPOOL_DATABASE_URL" \
  --treasury-address "$KATPOOL_POOL_ADDRESS" \
  --treasury-key-path "$KATPOOL_TREASURY_KEY_PATH" \
  > reconcile.json \
  2> reconcile.log
```

The binary writes:

- **stdout** — a single JSON envelope (`schema:
  katpool-krc20-rehearsal.reconcile/v1`) with the eligible-wallet snapshot,
  the planned cycle, the planned `krc20_pending_transfer` rows, the parent
  `payout` rows, the dry-run settle report (mass-planned + signed + verified,
  nothing recorded or sent), an empty credit report, and the cycle audit
  trail.
- **stderr** — structured `tracing` events.

## What success looks like

A clean rehearsal (go for enabling the engine):

1. **Exit code = 0.** See the exit-code table below.
2. **`dry_run == true`** and **`settle.commits_broadcast == 0`** and
   **`settle.reveals_broadcast == 0`** — every commit was mass-planned,
   signed, and verified, but nothing was recorded or broadcast.
3. **`settle.errors` is empty** — every selected transfer mass-planned and
   signed against the live treasury UTXO set. A non-empty list means the
   treasury is short for a commit, a commit/reveal exceeds the mass limit,
   or signing failed; read the entries and fix before a live run.
4. **`credit.credited == 0`** — dry-run never mutates `nacho_rebate.paid_sompi`.
5. **Every `transfers[].status == "pending"`** — dry-run never advances rows.
6. **`reconciled_status == "planned"`** — the cycle persisted as planned only.
7. **`audit[]` contains `krc20_cycle.plan` and `krc20_cycle.reconcile`** for
   this `cycle.id`; the `krc20_cycle.plan` payload carries
   `floor_price_mantissa` / `floor_price_scale` — the quoted price the
   conversion used.

```bash
jq '{exit_hint: .reconciled_status,
     settle_errors: (.settle.errors | length),
     commits: .settle.commits_broadcast,
     credited: .credit.credited,
     transfers: (.transfers | length),
     eligible: .eligible_wallets.count}' reconcile.json
```

## Exit codes

| Code | Meaning | Operator action |
|---|---|---|
| `0` | Dry-run planned cleanly; every selected transfer mass-planned + signed. | Evidence is go. Proceed to enable the engine. |
| `2` | Planned, but `settle.errors` is non-empty (treasury short for a commit, mass exceeded, or sign error). | Read `settle.errors` / `reconcile.log`; top up the treasury or fix the inputs; re-run. |
| `3` | Another instance holds the KRC-20 payout leader lock. | Stop the competing `katpool` instance (or wait), then re-run. Nothing was written. |
| other | Hard failure: kaspad connect, key load, DB, or floor-price quote. | Read `reconcile.log`; fix the environment; re-run. |

## What to do if a transfer cannot be planned (`settle.errors` non-empty)

The planner is mass-aware and funds each commit greedily from live UTXOs.
A failure is recorded per transfer rather than aborting the cycle, so one
underfunded recipient does not hide the rest.

```bash
jq '.settle.errors' reconcile.json
jq '.eligible_wallets | {count, total_pending_sompi}' reconcile.json
```

- **Underfunded treasury.** Fund the treasury address with headroom for
  every commit (`commit_amount_sompi` + fees per recipient) on testnet-10,
  then re-run.
- **Mass exceeded / sub-floor change.** Expected on pathological UTXO sets;
  see [ADR-0015](../decisions/0015-krc20-inscription-envelope.md) and the
  M5.3 planner. The rehearsal is idempotent: the same DAA window resumes the
  same cycle (`cycle.idempotency_key`), so re-running does not create a
  second cycle or re-select recipients.

## Restart / re-run semantics

The rehearsal is safe to re-run any number of times:

- It is always dry-run — no funds move, no txid is recorded, no row is
  advanced, and `nacho_rebate.paid_sompi` is never touched.
- The cycle is keyed by its DAA window (`krc20-<start>-<end>`); re-running
  inside the same window resumes the same planned cycle (and the same frozen
  recipients/amounts) rather than creating a duplicate.
- The advisory lock is released at the end of the run (and freed on process
  exit even if the run panics).

## Acceptance evidence

For the Phase 5 sign-off, archive the full artefact directory under
`payout-evidence/` in the release ticket:

1. `reconcile.json` — the JSON envelope (stdout).
2. `reconcile.log` — the tracing log (stderr).
3. `audit-log.txt` — the cycle's audit trail (extracted from the envelope).
4. `manifest.json` — git rev, binary sha256, timestamps, exit code,
   `cycle_id`, `reconciled_status`, `settle_errors`.

All four artefacts close [Phase 5 acceptance](../phase-5-acceptance.md)
row 6.
