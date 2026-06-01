# Runbook 18 — KAS payout dry-run rehearsal

A one-shot binary (`katpool-payout-rehearsal`) that drives **one dry-run
KAS payout cycle** through the production engine
(`payout_kas::PayoutEngine` in `ExecutionMode::DryRun`): it acquires the
single-leader advisory lock, derives the DAA cycle window, plans against
the **live** treasury UTXO set, signs and verifies every batch through the
txscript engine, and reconciles — **without broadcasting and without
marking any `payout` row submitted**. The planned cycle, the planned
`payout` rows, and the `cycle.plan` / `cycle.reconcile` audit trail are the
Phase 4 sign-off evidence.

See:

- [Phase 4 acceptance](../phase-4-acceptance.md) — rows 8 (engine) and 9 (this rehearsal)
- [`katpool-payout-rehearsal/src/`](../../katpool-payout-rehearsal/src) — the tool
- [Runbook 11 — Treasury key rotation](11-key-rotation.md) — how the treasury key is delivered in production
- [Runbook 13 — kaspad-tn10 bootstrap](13-kaspad-tn10-bootstrap.md) — the testnet-10 node this runs against

## When to use this runbook

Two triggers:

1. **Phase 4 acceptance sign-off.** Run once against testnet-10 with a
   funded treasury to produce the reconcile JSON + audit log + manifest
   that close acceptance row 9. Archive under `payout-evidence/`.
2. **Pre-enable smoke check.** Before flipping `KATPOOL_PAYOUT_ENABLED=true`
   (and later `KATPOOL_PAYOUT_DRY_RUN=false`) on any environment, run the
   rehearsal to confirm the treasury funds every eligible recipient and the
   plan signs/verifies cleanly.

This tool **never** broadcasts. Real payouts run inside the `katpool`
runtime via the engine; see [Phase 4 acceptance](../phase-4-acceptance.md).

## Preconditions

- A target Postgres with the new katpool schema migrated and at least one
  matured block's `share_allocation` rows present (otherwise no wallet is
  eligible and the cycle plans zero recipients — still a valid, if empty,
  rehearsal).
- A reachable testnet-10 kaspad gRPC endpoint (see Runbook 13).
- A funded treasury address on testnet-10 (the first `KATPOOL_POOL_ADDRESS`),
  and the matching raw 32-byte **hex** key in a file readable only by the
  operator. For testnet rehearsal the key is a hex file, not a systemd
  credential — production delivery is Runbook 11.
- No other `katpool` instance holding the payout leader lock on the same DB
  (the rehearsal shares the `payout-kas:kas-leader` advisory key; a live
  leader makes the rehearsal exit `3` without doing anything).
- `katpool-payout-rehearsal` built (`cargo build --release -p katpool-payout-rehearsal`).
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
# Optional: KATPOOL_PAYOUT_THRESHOLD_SOMPI, KATPOOL_PAYOUT_CYCLE_SPAN_DAA, KATPOOL_NETWORK
./scripts/kas-payout-rehearsal.sh
# → writes to ./payout-evidence/<UTC-stamp>-dry-run/
```

Lower-level invocation (CI / no `jq`):

```bash
katpool-payout-rehearsal \
  --kaspad-url "$KASPAD_GRPC_URL" \
  --database-url "$KATPOOL_DATABASE_URL" \
  --treasury-address "$KATPOOL_POOL_ADDRESS" \
  --treasury-key-path "$KATPOOL_TREASURY_KEY_PATH" \
  > reconcile.json \
  2> reconcile.log
```

The binary writes:

- **stdout** — a single JSON envelope (`schema:
  katpool-payout-rehearsal.reconcile/v1`) with the eligible-wallet snapshot,
  the planned cycle, the planned payout rows, the dry-run broadcast report
  (signed + verified, nothing sent), and the cycle audit trail.
- **stderr** — structured `tracing` events.

## What success looks like

A clean rehearsal (go for enabling the engine):

1. **Exit code = 0.** See the exit-code table below.
2. **`dry_run == true`** and **`broadcast.submitted_payouts == 0`** and
   **`broadcast.submitted_txids` is non-empty** — the plan was signed and a
   deterministic txid computed, but nothing was broadcast and no row was
   marked submitted.
3. **`broadcast.unpaid == 0`** — the treasury funded every eligible
   recipient. A non-zero value means the treasury balance is short; top it
   up before a live run.
4. **`broadcast.submit_errors` is empty.**
5. **Every `payouts[].status == "planned"`** — dry-run never advances rows.
6. **`reconciled_status == "planned"`** — the cycle persisted as planned
   only.
7. **`audit[]` contains `cycle.plan` and `cycle.reconcile`** for this
   `cycle.id` — the trail proves the cycle was created and reconciled.

```bash
jq '{exit_hint: .reconciled_status, unpaid: .broadcast.unpaid,
     submitted: .broadcast.submitted_payouts, batches: .broadcast.planned_batches,
     recipients: (.payouts | length)}' reconcile.json
```

## Exit codes

| Code | Meaning | Operator action |
|---|---|---|
| `0` | Dry-run planned cleanly; every eligible recipient funded. | Evidence is go. Proceed to enable the engine. |
| `2` | Planned, but `unpaid > 0` or a sign/verify error. | Top up the treasury or investigate the error in `reconcile.log`; re-run. |
| `3` | Another instance holds the payout leader lock. | Stop the competing `katpool` instance (or wait), then re-run. Nothing was written. |
| other | Hard failure: kaspad connect, key load, DB. | Read `reconcile.log`; fix the environment; re-run. |

## What to do if the treasury is underfunded (`unpaid > 0`)

The planner is mass-aware and funds recipients greedily from live UTXOs;
`unpaid` counts recipients it could not cover. This is expected on a fresh
testnet treasury.

```bash
jq '.eligible_wallets | {count, total_payable_sompi}' reconcile.json
```

Fund the treasury address with at least `total_payable_sompi` (plus fee
headroom) on testnet-10, then re-run. The rehearsal is idempotent: the same
DAA window resumes the same cycle (`cycle.idempotency_key`), so re-running
does not create a second cycle.

## Restart / re-run semantics

The rehearsal is safe to re-run any number of times:

- It is always dry-run — no funds move, no row is marked submitted.
- The cycle is keyed by its DAA window (`kas-<start>-<end>`); re-running
  inside the same window resumes the same planned cycle rather than creating
  a duplicate.
- The advisory lock is released at the end of the run (and freed on process
  exit even if the run panics).

## Acceptance evidence

For the Phase 4 sign-off, archive the full artefact directory under
`payout-evidence/` in the release ticket:

1. `reconcile.json` — the JSON envelope (stdout).
2. `reconcile.log` — the tracing log (stderr).
3. `audit-log.txt` — the cycle's audit trail (extracted from the envelope).
4. `manifest.json` — git rev, binary sha256, timestamps, exit code,
   `cycle_id`, `reconciled_status`, `unpaid`.

All four artefacts close [Phase 4 acceptance](../phase-4-acceptance.md)
row 9.
