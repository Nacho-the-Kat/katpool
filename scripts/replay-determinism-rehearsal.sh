#!/usr/bin/env bash
# Phase 3 M4: replay-determinism rehearsal wrapper.
#
# Runs dual-database determinism verification (same event stream →
# two independent Postgres instances → byte-equal snapshots) and
# optionally a single-database replay for operator evidence.
#
# See docs/runbooks/17-replay-determinism.md.
#
# Required for dual-verify (default):
#   - Docker (two ephemeral postgres:17-alpine containers)
#   - `cargo test -p accountant --test replay_harness_scale` OR
#     pre-built workspace tests
#
# Optional single-DB replay:
#   KATPOOL_DATABASE_URL  migrated throwaway Postgres
#   --events PATH         NDJSON PoolEvent log
#   --legacy-log PATH     legacy monitoring log
#
# Outputs (when REHEARSAL_OUTPUT_DIR set or default):
#   <out>/manifest.json   git rev, timestamps, test result
#   <out>/verify.log      cargo test stderr

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

NOTE="${REHEARSAL_NOTE:-replay-determinism}"
STAMP="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
OUT_DIR="${REHEARSAL_OUTPUT_DIR:-$REPO_ROOT/replay-evidence/${STAMP}-${NOTE}}"
mkdir -p "$OUT_DIR"

GIT_REV="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
STARTED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo "=== M4 dual-replay determinism (CI harness test) ===" | tee "$OUT_DIR/verify.log"
if ! cargo test -p accountant --test replay_harness_scale -- --nocapture 2>&1 | tee -a "$OUT_DIR/verify.log"; then
  FINISHED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  cat >"$OUT_DIR/manifest.json" <<EOF
{
  "schema": "katpool-replay-rehearsal.manifest/v1",
  "git_rev": "$GIT_REV",
  "timestamps": { "started": "$STARTED", "finished": "$FINISHED" },
  "dual_verify": "failed",
  "note": "$NOTE"
}
EOF
  exit 1
fi

FINISHED="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
cat >"$OUT_DIR/manifest.json" <<EOF
{
  "schema": "katpool-replay-rehearsal.manifest/v1",
  "git_rev": "$GIT_REV",
  "timestamps": { "started": "$STARTED", "finished": "$FINISHED" },
  "dual_verify": "passed",
  "ci_harness": "accountant/tests/replay_harness_scale.rs",
  "note": "$NOTE"
}
EOF

# Optional operator replay against supplied inputs.
if [[ -n "${KATPOOL_DATABASE_URL:-}" ]]; then
  REPLAY_BIN="${KATPOOL_REPLAY_BIN:-$REPO_ROOT/target/release/katpool-replay}"
  ARGS=(--database-url "$KATPOOL_DATABASE_URL" --emit-summary)
  [[ -n "${KATPOOL_NETWORK:-}" ]] && ARGS+=(--network "$KATPOOL_NETWORK")
  for arg in "$@"; do ARGS+=("$arg"); done
  if [[ ${#ARGS[@]} -gt 3 ]]; then
    echo "=== single-database operator replay ===" | tee -a "$OUT_DIR/verify.log"
    "$REPLAY_BIN" "${ARGS[@]}" 2>&1 | tee -a "$OUT_DIR/replay-summary.json"
  fi
fi

echo "artefacts: $OUT_DIR"
exit 0
