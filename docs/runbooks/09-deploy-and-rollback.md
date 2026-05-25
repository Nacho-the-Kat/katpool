# Runbook 09 — Deploy and rollback

## When to use

- Normal feature/bugfix deploy after a merge to `main`
- Emergency rollback during or just after a problematic deploy
- Manual recovery if the deploy workflow itself misbehaves

## Deploy procedure (normal)

Triggered automatically by a merge to `main` after CI passes
([`release.yml`](../../.github/workflows/release.yml)). The
workflow:

1. Builds a static musl binary
2. Generates SBOM with `syft` (CycloneDX JSON)
3. Signs the binary and SBOM with `cosign` (keyless via OIDC)
4. Pushes the signed Docker image with a pinned digest
5. Opens a deploy PR (or auto-triggers the production deploy if
   the workflow is configured for it)
6. Deploy script on the VPS verifies the cosign signature before
   activating

To trigger manually (e.g. re-deploy of an already-built artifact):

```bash
gh workflow run release.yml --ref main
```

To deploy to the VPS without going through CI (emergency only):

```bash
ssh prod-vps /opt/katpool/deploy/deploy.sh <signed-image-digest>
```

The script:

- Pulls the image by digest
- Verifies cosign signature; refuses to proceed if missing
- Runs migrations (idempotent)
- `systemctl reload katpool` (zero-downtime hot reload for
  config; full restart only if the binary changed)
- Waits for `/health` and `/ready` to return 200
- Marks the deploy successful in the deploy log

## Rollback procedure

If a deploy causes alerts to fire, or if the operator's judgement
says "back this out":

```bash
ssh prod-vps /opt/katpool/deploy/rollback.sh
```

This script:

1. Looks up the previous signed image digest from the deploy log
2. Re-pulls and verifies its signature
3. Rolls back any DB migrations applied by the failed deploy
   (each migration has a `down` step; the script applies them
   in reverse order, gated by an interactive confirmation)
4. Restarts `katpool` against the previous digest
5. Waits for `/health` and `/ready` to return 200
6. Marks the rollback in the deploy log

If the rollback script itself fails (e.g. the down-migration
errored), this is a manual recovery scenario:

1. Stop the pool: `systemctl stop katpool`
2. Identify the last-known-good database state (via pgBackRest
   PITR — see [04](04-postgres-restore-from-backup.md))
3. Restore the database to just before the failed deploy
4. Start the pool against the previous binary digest
5. File a SEV-2 incident; do not deploy again until the issue
   is understood

## Verification after deploy or rollback

- `/health` and `/ready` return 200
- `katpool_started_total` counter increments by 1 (proves the
  process actually restarted)
- No alerts firing
- Canary miner shares are being credited
- A test query against the API returns expected data
- Migrations applied or rolled back successfully (verify schema
  version table)

## When NOT to deploy

- During an active incident (unless the deploy is the
  mitigation)
- Within 1 h of a scheduled payout cycle
- Within 24 h of a Kaspa hardfork without explicit testnet
  validation
- Without a passing CI and signed commit on `main`

## Audit trail

Every deploy and rollback writes to
`/var/log/katpool/deploy.jsonl` (also shipped to Loki). Includes:

- Deploy timestamp
- Image digest before + after
- Migration versions before + after
- Operator's GitHub identity (from the OIDC token)
- Success/failure
- Any down-migration applied during rollback

This is the source of truth for "who deployed what when".
