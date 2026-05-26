# Runbook 14 — run the legacy-database importer

A one-shot binary (`katpool-import-legacy`) that copies the previous
generation's PostgreSQL state into the new katpool schema. Designed
to be run **once during cutover** but engineered idempotent so that
a partial-failure restart is safe and a dry-run pre-cutover
rehearsal is trivial.

See:

- [Phase 2 schema reference](../db-schema.md)
- [`katpool-import-legacy/src/`](../../katpool-import-legacy/src) — the actual transform code
- [ADR-0007 — schema-first DB design](../decisions/0007-postgres-schema-first.md) (file path TBD until ADR lands)

## When to use this runbook

Three triggers:

1. **Cutover dry-run** (T-24h before cutover). Run with
   `--dry-run` against production-snapshot copies of both
   databases. Outcome: a clean reconcile report + signed-off
   classifier counts.
2. **Cutover hot-run** (during the migration maintenance window).
   Run for real against the legacy DB while the legacy stack is
   stopped. Outcome: the new schema is populated and the reconcile
   pass passes.
3. **Restart after partial failure**. The importer crashed
   mid-run. Re-invoke the same command — every transform is
   idempotent (UPSERT, ON CONFLICT DO NOTHING, set-not-add) so the
   restart picks up cleanly.

## Preconditions

- A target Postgres with the new katpool schema migrated. The
  importer asserts `migrate::run` separately; this runbook assumes
  the target DB has already had `sqlx migrate run` (or equivalent)
  applied.
- A legacy Postgres with the production `katpool_mainnet` schema.
- `katpool-import-legacy` binary available
  (`cargo build --release -p katpool-import-legacy`).
- The operator has `psql` available for the spot-check queries
  below.

## Command

Use the wrapper script — it captures the JSON envelope, the
tracing log, the audit-log snapshot, and a manifest with the
git rev + binary sha256 into a timestamped artefact directory.
Required for the cutover ticket.

```bash
# T-24h dry-run rehearsal (against a snapshot of production).
export LEGACY_DATABASE_URL='postgres://katpool_ro@legacy-snapshot/katpool_mainnet'
export KATPOOL_DATABASE_URL='postgres://katpool_rw@new-snapshot/katpool'
./scripts/legacy-importer-rehearsal.sh
# → writes to ./cutover-evidence/<UTC-stamp>-dry-run/
```

```bash
# Cutover hot-run (legacy stack STOPPED, write traffic frozen).
export LEGACY_DATABASE_URL="$LEGACY_DATABASE_URL"
export KATPOOL_DATABASE_URL="$KATPOOL_DATABASE_URL"
./scripts/legacy-importer-rehearsal.sh --no-dry-run
# → writes to ./cutover-evidence/<UTC-stamp>-hot-run/
```

Lower-level invocation (only when the wrapper script can't be
used, e.g. in a CI environment without `psql`/`jq`):

```bash
katpool-import-legacy \
  --source "$LEGACY_DATABASE_URL" \
  --target "$KATPOOL_DATABASE_URL" \
  [--dry-run] \
  > reconcile.json \
  2> reconcile.log
```

The binary writes:

- **stdout** — a single JSON envelope with per-transform counts +
  the full reconciliation report. Operators capture this for the
  cutover audit log.
- **stderr** — structured `tracing` events, one per row decision
  (insert / skip / reject). Verbose; useful for postmortem.

## What success looks like

Every line below must be true before cutover proceeds:

1. **Exit code = 0.** Non-zero (specifically `2`) means at least
   one reconcile check failed.
2. **`reconcile.all_passed == true`** in the stdout JSON.
3. **For every transform:** `rejected == 0`. Any rejected row is a
   data-quality bug in the legacy DB — investigate via the stderr
   log before re-running.
4. **`payments.amount_total_sompi`** (legacy) **==**
   `sum(amount_sompi)` of `kas-legacy-*` cycles' payouts (new).
5. **`nacho_payments.amount_total`** (legacy) **==**
   `sum(amount_sompi)` of `krc20-legacy-*` cycles' payouts (new)
   **excluding** `krc20-legacy-pending-*`.
6. **`miners_balance.nacho_rebate_total`** (legacy) **==**
   `sum(accrued_sompi)` (new).
7. **Per-status counts** of `krc20_pending_transfer` match the
   legacy `pending_krc20_transfers` rows.

The reconciliation pass enforces all of (4)–(7) and short-circuits
the operator on any mismatch. The cutover continues only if every
check is green.

## What to do if a reconcile check fails

```bash
jq '.reconcile.checks[] | select(.passed == false)' reconcile-cutover.json
```

For each failed check:

- **`blocks.row_count`** mismatch → some `block_details` rows had
  unparseable `mined_block_hash`. Grep stderr for `"blocks row rejected"`.
- **`payments.amount_total_sompi`** mismatch → some `payments` rows
  had invalid `transaction_hash` (rejected) or invalid recipient
  wallet (rejected). Grep stderr for `"payments row rejected"`.
- **`miners_balance.nacho_rebate_total`** mismatch → some
  `miners_balance` rows had negative or overflow rebate values, or
  invalid wallets. Grep stderr for `"miners_balance row rejected"`.
- **`krc20_pending_transfer.count[*]`** mismatch → rejected rows in
  `pending_krc20_transfers`. Grep stderr for
  `"pending_krc20_transfers row rejected"`.

For each, decide:

1. **Data-quality bug in legacy that we can ignore.** Note the
   number of rejected rows in the cutover audit log; proceed if
   the rejected-set is bounded and explicable.
2. **Bug in the importer.** Roll back the cutover (the target DB
   is fresh — nothing to roll back beyond stopping the importer).
   File a `bug:importer` issue, fix, re-test against the snapshot,
   schedule a new cutover window.

## Out-of-band: pending legacy KAS balance

The importer **intentionally does not** copy
`miners_balance.balance` (the pending KAS balance). The new
schema computes KAS payable balance from per-block
`share_allocation.net_payout_sompi`, which the legacy schema
never tracked.

The cutover plan therefore requires that the **legacy pool** flush
every remaining `balance` row as its final on-chain action — i.e.,
emit a final KAS payout cycle that zeros out every pending balance.
Cutover does not proceed until that on-chain transaction confirms.

The reconciliation pass does **not** check this — it's an operator
precondition, not an importer concern.

## Restart after partial failure

If the importer crashes or is killed mid-run:

```bash
# Re-invoke the exact same command. Every transform is
# idempotent (UPSERT / ON CONFLICT DO NOTHING / set-not-add).
katpool-import-legacy --source ... --target ...
```

The second run will report most rows as `skipped` (already
imported) and only insert rows from the legacy table that the
first run hadn't reached yet. The reconcile pass at the end will
prove convergence.

## Performance envelope

On a snapshot of the production legacy DB (~1.5M `block_details`,
~30K `payments`, ~12K `nacho_payments`, ~5K
`pending_krc20_transfers`, ~2.6K `miners_balance`), end-to-end
import + reconciliation completes in **under 15 minutes** on the
production VPS. We allocate a 30-minute cutover budget for the
importer with a 30-minute contingency.

## Cutover audit trail

The operator running the importer is responsible for collecting:

1. The full `reconcile-cutover.json` (stdout).
2. The full `reconcile-cutover.log` (stderr).
3. A snapshot of the new schema's `audit_log` table at T+0 (right
   after the importer exits). All importer transforms write to the
   audit log; the snapshot proves the import is consistent with
   the cycle/payout state at cutover time.

All three artifacts go into the cutover release ticket.
