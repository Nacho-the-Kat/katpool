#!/usr/bin/env bash
# Phase 2 milestone 4: cutover-rehearsal wrapper around
# `katpool-import-legacy`.
#
# Intended for two operator-facing scenarios documented in
# `docs/runbooks/14-legacy-importer.md`:
#
#   1. T-24h dry-run rehearsal (default mode).
#      Captures the reconcile JSON as a go/no-go artefact for the
#      cutover ticket.
#
#   2. Cutover hot-run (`--no-dry-run`).
#      Same script, same outputs; only difference is the importer
#      writes for real.
#
# Required environment:
#   - LEGACY_DATABASE_URL  postgres URL of the legacy database
#     (read-only role recommended for dry-run).
#   - KATPOOL_DATABASE_URL postgres URL of the new (target) DB
#     (must have the new schema migrated).
#
# Optional:
#   - KATPOOL_IMPORT_BIN   path to the importer binary. Default:
#                          `katpool-import-legacy` on PATH.
#   - REHEARSAL_OUTPUT_DIR directory to write artefacts into.
#                          Default: ./cutover-evidence/<UTC-ISO8601>
#   - REHEARSAL_NOTE       short note appended to the artefact dir
#                          name (e.g., "dry-run", "hot-run").
#
# Outputs:
#   - <out>/reconcile.json    stdout of the importer (JSON envelope)
#   - <out>/reconcile.log     stderr (tracing events; for postmortem)
#   - <out>/manifest.json     metadata: git rev, binary path/sha256,
#                             timestamps, exit code, env summary
#   - <out>/audit-log.txt     snapshot of the new schema's audit_log
#                             table at T+0 (rows since the importer
#                             started). Cutover evidence: required.
#
# Exit code:
#   0  importer succeeded AND reconcile.all_passed == true
#   2  reconcile mismatch (importer wrote, but cross-checks failed)
#   *  any other failure (missing prereqs, importer crash, etc.)
#
# This script is intentionally simple: it doesn't try to be a
# replacement for the runbook. It produces deterministic artefacts
# that the runbook tells the operator what to do with.

set -euo pipefail

# -------- arg parsing (does not need env) --------------------------
DRY_RUN_FLAG="--dry-run"
NOTE_DEFAULT="dry-run"
for arg in "$@"; do
  case "$arg" in
    --no-dry-run)
      DRY_RUN_FLAG=""
      NOTE_DEFAULT="hot-run"
      ;;
    --help|-h)
      sed -n '1,60p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      exit 1
      ;;
  esac
done

# -------- pre-flight (env-dependent) -------------------------------
require_var() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "FATAL: $name is required (see docs/runbooks/14-legacy-importer.md)" >&2
    exit 1
  fi
}

require_var LEGACY_DATABASE_URL
require_var KATPOOL_DATABASE_URL

IMPORT_BIN=${KATPOOL_IMPORT_BIN:-katpool-import-legacy}
if ! command -v "$IMPORT_BIN" >/dev/null 2>&1; then
  if [[ -x "$IMPORT_BIN" ]]; then
    :
  else
    echo "FATAL: importer binary '$IMPORT_BIN' not found on PATH and not executable" >&2
    exit 1
  fi
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "FATAL: jq is required for manifest assembly" >&2
  exit 1
fi
if ! command -v psql >/dev/null 2>&1; then
  echo "FATAL: psql is required for audit-log snapshot" >&2
  exit 1
fi
if ! command -v sha256sum >/dev/null 2>&1; then
  echo "FATAL: sha256sum is required for binary attestation" >&2
  exit 1
fi

NOTE=${REHEARSAL_NOTE:-$NOTE_DEFAULT}
STAMP=$(date -u +%Y-%m-%dT%H-%M-%SZ)
OUTDIR=${REHEARSAL_OUTPUT_DIR:-./cutover-evidence/${STAMP}-${NOTE}}
mkdir -p "$OUTDIR"

JSON_OUT="$OUTDIR/reconcile.json"
LOG_OUT="$OUTDIR/reconcile.log"
MANIFEST="$OUTDIR/manifest.json"
AUDIT_OUT="$OUTDIR/audit-log.txt"

GIT_REV=$(git -C "$(dirname "$0")/.." rev-parse HEAD 2>/dev/null || echo "unknown")
BIN_PATH=$(command -v "$IMPORT_BIN" || readlink -f "$IMPORT_BIN")
BIN_SHA=$(sha256sum "$BIN_PATH" | awk '{print $1}')
STARTED_AT=$(date -u --iso-8601=seconds)
T0_EPOCH=$(date -u +%s)

echo "==> katpool-import-legacy rehearsal" >&2
echo "    out=$OUTDIR" >&2
echo "    mode=${DRY_RUN_FLAG:-hot-run}" >&2
echo "    binary=$BIN_PATH (sha256=${BIN_SHA:0:16}…)" >&2
echo "    git=$GIT_REV" >&2

# -------- run importer ---------------------------------------------
set +e
"$IMPORT_BIN" \
  --source-url "$LEGACY_DATABASE_URL" \
  --target-url "$KATPOOL_DATABASE_URL" \
  $DRY_RUN_FLAG \
  > "$JSON_OUT" \
  2> "$LOG_OUT"
IMPORTER_EXIT=$?
set -e

FINISHED_AT=$(date -u --iso-8601=seconds)

# -------- audit-log snapshot ---------------------------------------
# Capture every audit_log row written by the importer (rows whose
# `created_at` is >= the T0 epoch we recorded above).
psql "$KATPOOL_DATABASE_URL" -At -F $'\t' -c \
  "SELECT id, created_at, subject_table, subject_id, event_kind, actor, payload::text
     FROM audit_log
    WHERE created_at >= to_timestamp(${T0_EPOCH})
    ORDER BY id ASC" \
  > "$AUDIT_OUT" 2>/dev/null || true

# -------- manifest -------------------------------------------------
RECONCILE_PASS="unknown"
if jq -e '.reconcile.all_passed == true' "$JSON_OUT" >/dev/null 2>&1; then
  RECONCILE_PASS="true"
elif jq -e '.reconcile.all_passed == false' "$JSON_OUT" >/dev/null 2>&1; then
  RECONCILE_PASS="false"
fi

jq -n \
  --arg git_rev "$GIT_REV" \
  --arg bin_path "$BIN_PATH" \
  --arg bin_sha "$BIN_SHA" \
  --arg started "$STARTED_AT" \
  --arg finished "$FINISHED_AT" \
  --arg mode "${DRY_RUN_FLAG:-hot-run}" \
  --arg note "$NOTE" \
  --argjson exit "$IMPORTER_EXIT" \
  --arg reconcile_pass "$RECONCILE_PASS" \
  '{
    schema: "katpool-import-legacy.rehearsal-manifest/v1",
    git_rev: $git_rev,
    binary: { path: $bin_path, sha256: $bin_sha },
    timestamps: { started: $started, finished: $finished },
    mode: $mode,
    note: $note,
    importer_exit_code: $exit,
    reconcile_all_passed: $reconcile_pass
  }' \
  > "$MANIFEST"

# -------- final disposition ----------------------------------------
echo "==> rehearsal complete" >&2
echo "    importer_exit=$IMPORTER_EXIT  reconcile_all_passed=$RECONCILE_PASS" >&2
echo "    artefacts:" >&2
ls -la "$OUTDIR" >&2

# Surface the same exit code that the importer surfaced so a
# caller (Makefile, CI, the on-call cutover script) can branch on
# it without parsing JSON.
exit $IMPORTER_EXIT
