# Runbooks

One runbook per incident class. Each runbook is the on-call's first
stop when the corresponding alert fires.

| Runbook | Topic | Alert(s) |
|---|---|---|
| [00](00-on-call-overview.md) | On-call overview, comms, first 5 minutes | — |
| [01](01-blocks-stopped-being-found.md) | Blocks stopped being found | `BlocksNotFound` |
| [02](02-nacho-payout-failed.md) | NACHO payout failed | `NachoPayoutFailed` |
| [03](03-kaspad-lost-peers.md) | kaspad lost peers | `KaspadPeerCountLow` |
| [04](04-postgres-restore-from-backup.md) | Postgres restore from backup | manual, or chained from a DR-validator failure |
| [05](05-treasury-balance-below-threshold.md) | Treasury balance below threshold | `TreasuryBalanceLow` |
| [06](06-miner-visible-outage.md) | Miner-visible outage | `HealthEndpointDown` |
| [07](07-storage-mass-rejection-burst.md) | Storage-mass rejection burst | `StorageMassRejectionBurst` |
| [08](08-stratum-flood-or-abuse.md) | Stratum flood / abuse | `StratumAbuse` |
| [09](09-deploy-and-rollback.md) | Deploy and rollback | — (manual procedure) |
| [10](10-automated-dr-validation.md) | Automated DR validation | `DRValidatorMissed`, `DRValidatorFailed` |
| [11](11-key-rotation.md) | Treasury key rotation | — (scheduled drill or compromise response) |

Each runbook follows the same structure: Symptom → Confirm → Diagnose
→ Remediate → Verify → Post-incident. If a runbook deviates from this
shape, that's the runbook's fault, not yours.

Runbooks evolve. If you ran one during an incident and something was
wrong, missing, or stale, **the next thing you do after the incident
is open a PR fixing it** — before the postmortem, even. The runbook
exists for the next person.
