#!/usr/bin/env bash
# Phase 4 milestone 8: KAS payout dry-run rehearsal wrapper around
# `katpool-payout-rehearsal`.
#
# Drives ONE dry-run payout cycle against a (testnet-10) kaspad + the
# new-schema Postgres: plan + sign + verify against the live treasury
# UTXO set, no broadcast, no rows marked submitted. Captures the
# reconcile JSON, the tracing log, the cycle audit trail, and a
# manifest into a timestamped evidence directory for the Phase 4
# sign-off (see docs/runbooks/18-kas-payout-rehearsal.md).
#
# The tool is ALWAYS dry-run; there is no hot-run mode here. Real
# payouts run inside the `katpool` runtime via KATPOOL_PAYOUT_ENABLED.
#
# Required environment (consumed by the binary via clap `env`):
#   - KASPAD_GRPC_URL          kaspad gRPC URL (grpc://host:port)
#   - KATPOOL_DATABASE_URL     new-schema Postgres URL (migrated)
#   - KATPOOL_TREASURY_KEY_PATH  raw 32-byte hex treasury key file
#   - KATPOOL_POOL_ADDRESS     treasury (pool) address
#
# Optional:
#   - KATPOOL_NETWORK                schema-network label (else derived)
#   - KATPOOL_PAYOUT_THRESHOLD_SOMPI eligibility threshold (default 5 KAS)
#   - KATPOOL_PAYOUT_CYCLE_SPAN_DAA  cycle DAA span (default 86_400)
#   - KATPOOL_PAYOUT_REHEARSAL_BIN   path to the binary. Default:
#                                    `katpool-payout-rehearsal` on PATH.
#   - REHEARSAL_OUTPUT_DIR           artefact dir. Default:
#                                    ./payout-evidence/<UTC-stamp>-<note>
#   - REHEARSAL_NOTE                 dir-name suffix. Default "dry-run".
#
# Outputs:
#   - <out>/reconcile.json   stdout of the tool (JSON envelope)
#   - <out>/reconcile.log    stderr (tracing events; postmortem)
#   - <out>/audit-log.txt    the cycle's audit trail (extracted from JSON)
#   - <out>/manifest.json    git rev, binary sha256, timestamps, exit code,
#                            cycle id, reconciled status, unpaid count
#
# Exit code (surfaced from the binary):
#   0  dry-run planned cleanly — every eligible recipient funded
#   2  planned but treasury underfunded (unpaid > 0) or a sign error
#   3  another instance holds the payout leader lock; nothing ran
#   *  hard failure (connect, key load, kaspad RPC)

set -euo pipefail

# -------- arg parsing ----------------------------------------------
for arg in "$@"; do
  case "$arg" in
    --help|-h)
      sed -n '1,55p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      exit 1
      ;;
  esac
done

# -------- pre-flight -----------------------------------------------
require_var() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "FATAL: $name is required (see docs/runbooks/18-kas-payout-rehearsal.md)" >&2
    exit 1
  fi
}

require_var KASPAD_GRPC_URL
require_var KATPOOL_DATABASE_URL
require_var KATPOOL_TREASURY_KEY_PATH
require_var KATPOOL_POOL_ADDRESS

REHEARSAL_BIN=${KATPOOL_PAYOUT_REHEARSAL_BIN:-katpool-payout-rehearsal}
if ! command -v "$REHEARSAL_BIN" >/dev/null 2>&1 && [[ ! -x "$REHEARSAL_BIN" ]]; then
  echo "FATAL: rehearsal binary '$REHEARSAL_BIN' not found on PATH and not executable" >&2
  exit 1
fi

for tool in jq sha256sum; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "FATAL: $tool is required" >&2
    exit 1
  fi
done

NOTE=${REHEARSAL_NOTE:-dry-run}
STAMP=$(date -u +%Y-%m-%dT%H-%M-%SZ)
OUTDIR=${REHEARSAL_OUTPUT_DIR:-./payout-evidence/${STAMP}-${NOTE}}
mkdir -p "$OUTDIR"

JSON_OUT="$OUTDIR/reconcile.json"
LOG_OUT="$OUTDIR/reconcile.log"
AUDIT_OUT="$OUTDIR/audit-log.txt"
MANIFEST="$OUTDIR/manifest.json"

GIT_REV=$(git -C "$(dirname "$0")/.." rev-parse HEAD 2>/dev/null || echo "unknown")
BIN_PATH=$(command -v "$REHEARSAL_BIN" || readlink -f "$REHEARSAL_BIN")
BIN_SHA=$(sha256sum "$BIN_PATH" | awk '{print $1}')
STARTED_AT=$(date -u --iso-8601=seconds)

echo "==> katpool-payout-rehearsal (dry-run)" >&2
echo "    out=$OUTDIR" >&2
echo "    binary=$BIN_PATH (sha256=${BIN_SHA:0:16}…)" >&2
echo "    git=$GIT_REV" >&2

# -------- run rehearsal --------------------------------------------
set +e
"$REHEARSAL_BIN" > "$JSON_OUT" 2> "$LOG_OUT"
REHEARSAL_EXIT=$?
set -e

FINISHED_AT=$(date -u --iso-8601=seconds)

# -------- audit trail (from the binary's own envelope; no psql) ----
jq '.audit // []' "$JSON_OUT" > "$AUDIT_OUT" 2>/dev/null || echo "[]" > "$AUDIT_OUT"

# -------- manifest -------------------------------------------------
CYCLE_ID=$(jq -r '.cycle.id // "null"' "$JSON_OUT" 2>/dev/null || echo "null")
RECONCILED=$(jq -r '.reconciled_status // "unknown"' "$JSON_OUT" 2>/dev/null || echo "unknown")
UNPAID=$(jq -r '.broadcast.unpaid // "null"' "$JSON_OUT" 2>/dev/null || echo "null")
ELIGIBLE=$(jq -r '.eligible_wallets.count // "null"' "$JSON_OUT" 2>/dev/null || echo "null")

jq -n \
  --arg git_rev "$GIT_REV" \
  --arg bin_path "$BIN_PATH" \
  --arg bin_sha "$BIN_SHA" \
  --arg started "$STARTED_AT" \
  --arg finished "$FINISHED_AT" \
  --arg note "$NOTE" \
  --argjson exit "$REHEARSAL_EXIT" \
  --arg cycle_id "$CYCLE_ID" \
  --arg reconciled "$RECONCILED" \
  --arg unpaid "$UNPAID" \
  --arg eligible "$ELIGIBLE" \
  '{
    schema: "katpool-payout-rehearsal.rehearsal-manifest/v1",
    git_rev: $git_rev,
    binary: { path: $bin_path, sha256: $bin_sha },
    timestamps: { started: $started, finished: $finished },
    mode: "dry-run",
    note: $note,
    rehearsal_exit_code: $exit,
    cycle_id: $cycle_id,
    reconciled_status: $reconciled,
    eligible_wallets: $eligible,
    unpaid: $unpaid
  }' \
  > "$MANIFEST"

# -------- final disposition ----------------------------------------
echo "==> rehearsal complete" >&2
echo "    exit=$REHEARSAL_EXIT  cycle_id=$CYCLE_ID  reconciled=$RECONCILED  unpaid=$UNPAID" >&2
echo "    artefacts:" >&2
ls -la "$OUTDIR" >&2

exit $REHEARSAL_EXIT
