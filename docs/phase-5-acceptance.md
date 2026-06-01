# Phase 5 acceptance evidence

Phase 5 (NACHO / KRC-20 rebate payout engine) closes when every row below
is GREEN for a release-candidate commit. It reuses the Phase 4 payout
scaffolding (`payout_cycle` / `payout` rows, `katpool-idempotency`
advisory lock, the `PayoutEngine` loop shape, `katpool-secrets` treasury
custody, and the `katpool-storagemass` planner).

Prerequisites: Phase 4 complete (KAS payout engine + treasury custody).
The `krc20_pending_transfer` table and `krc20_transfer_status` enum landed
in Phase 2 (`docs/db-schema.md`).

## Acceptance matrix

| # | Criterion | Verification | Status |
|---|---|---|---|
| 1 | KRC-20 inscription envelope: build the kasplex commit redeem script, P2SH commit address, and reveal signature script, byte-compatible with the kasplex-accepted production transfer. | Deterministic, chain-free unit tests pinning the exact envelope bytes, canonical compact JSON field order, testnet-10 P2SH derivation, hash-binds-payload, and `<sig><pushed redeem>` reveal script. Format decision recorded in [ADR-0015](decisions/0015-krc20-inscription-envelope.md). | GREEN — M5.1 |
| 2 | NACHO eligibility + rebate amount: accrued `nacho_rebate_kas` per wallet; floor-price quote from `api.kaspa.com` with a circuit breaker; full-rebate (3×) when holder has ≥ 100M NACHO **or** owns a KATCLAIM L3 NFT (token id 736..=843). Fixes the legacy `checkFullFeeRebate` truthiness bug. | Pure-function tests for the rebate/eligibility decision incl. the boundary cases the legacy bug got wrong; circuit-breaker unit tests; quote fetch behind a mockable trait. | PENDING — M5.2 |
| 3 | Mass-aware commit/reveal planner: one recipient per reveal; every planned commit and reveal tx satisfies independent mass ≤ `max_block_mass` incl. `transient_storage_mass`. | Property/unit tests via `katpool-storagemass` (`docs/kips.md` §5.2). | PENDING — M5.3 |
| 4 | Sign + submit commit/reveal via kaspad gRPC; drive `krc20_pending_transfer` (`pending → commit_submitted → reveal_submitted → completed` / `failed`); record intent **before** broadcast; no double-pay on mid-cycle restart. | Deterministic txscript-engine verification + mock-kaspad orchestration on testcontainer Postgres; crash-before-broadcast chaos test. | PENDING — M5.4 |
| 5 | `payout-krc20` wired into `katpool` runtime (periodic loop + distributed lock, reusing the Phase 4 engine shape); dry-run flag for rehearsal; safe-by-default. | Advisory-lock mutual-exclusion + multi-tick settlement / non-leader skip / shutdown tests over testcontainer Postgres + mock kaspad. | PENDING — M5.5 |
| 6 | Operator rehearsal: one full dry-run NACHO cycle produces reconcile JSON + audit log + manifest (mirrors the Phase 4 KAS rehearsal). | One-shot rehearsal tool + script + runbook; reconcile-envelope unit-tested; live testnet-10 dry-run archived under `payout-evidence/`. | PENDING — M5.6 |
| 7 | `cargo deny check` clean on the locked `Cargo.lock`. | CI step. | GREEN — inherited |

## Milestone map (PR-sized)

| Milestone | Delivers | Closes rows |
|---|---|---|
| **M5.1** | kasplex inscription envelope + P2SH + reveal-script primitives (pure) | 1 |
| **M5.2** | NACHO eligibility + floor-price quote + full-rebate logic | 2 |
| **M5.3** | Mass-aware commit/reveal planner | 3 |
| **M5.4** | kaspad commit/reveal sign/submit/confirm + transfer state machine | 4 |
| **M5.5** | `payout-krc20` engine + `katpool` wiring + dry-run | 5 |
| **M5.6** | Rehearsal tool + runbook + acceptance evidence | 6 |

## Out of scope for Phase 5

- **Phase 6** — Public HTTP API.
- **Phase 7–10** — Production edge, shadow run, cutover.

## Sign-off

Phase 5 closes when:

1. Every row in the matrix is GREEN.
2. A testnet-10 dry-run NACHO cycle completes (planned commit/reveal pairs +
   mass plan, no broadcast).
3. A live testnet-10 reveal credits the recipient's KRC-20 balance via the
   kasplex API (the empirical confirmation of ADR-0015).
4. Operator has archived rehearsal evidence under `payout-evidence/`.
