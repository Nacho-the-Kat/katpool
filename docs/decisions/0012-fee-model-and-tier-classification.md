---
status: accepted
date: 2026-05-26
deciders: argonmining
---

# ADR-0012: Pool fee model and wallet-tier classification

## Context and Problem Statement

The pool runs a two-tier fee model inherited from the legacy stack
and required to remain operationally identical:

- **Standard miners.** Charged the topline fee (default 0.75%);
  receive 33% of that fee back as NACHO tokens at the next krc-20
  payout cycle. Effective KAS net: `gross × (1 − topline)`.
- **Elite miners.** Same topline fee; receive 100% of it back as
  NACHO tokens. Effective KAS net: `gross × (1 − topline)`.

A miner qualifies as Elite by either:
1. Owning at least one token from the `NACHO` KRC-721 collection
   on Kasplex, **or**
2. Holding at least 100,000,000 NACHO tokens (at the KRC-20
   standard 8-decimal precision = 10¹⁶ base units).

The new accountant must reproduce this model exactly while making
the topline fee operator-tunable for future flexibility, keeping
the rebate ratios fixed for now, and never silently changing
historical allocations when the operator does turn the knob.

## Decision

### 1. Topline fee is operator-tunable via env

`KATPOOL_FEE_TOPLINE_BPS` (basis points integer; 75 = 0.75%; default
75). Bounded at `MAX_TOPLINE_BPS = 1 000 bps = 10%` to defend against
operator typos. Loaded once at boot; held read-only thereafter. The
basis-points integer representation eliminates the
"`0.75` vs `0.0075`" foot-gun in float-config.

### 2. Rebate ratios are fixed in code

`STANDARD_REBATE_BPS = 3 300` (33% of the fee) and
`ELITE_REBATE_BPS = 10 000` (100% of the fee). Changing either is a
code change, not an ops change. This is deliberate: rebate ratios
are part of the public contract with miners and shouldn't be
silently rotatable via env.

### 3. Per-block allocation math (integer end-to-end)

```text
fee_share     = gross * topline_bps / 10_000
nacho_accrual = fee_share * rebate_bps / 10_000
pool_fee      = fee_share - nacho_accrual
net_payout    = gross - fee_share
```

The balance equation `gross = pool_fee + nacho_accrual + net_payout`
holds by construction and is re-enforced server-side by
`share_allocation_balance` CHECK.

### 4. NACHO accrual stays in KAS-sompi

Every internal table — `share_allocation.nacho_accrual_sompi`,
`nacho_rebate_accrual.accrued_sompi` — stores rebate amounts in
sompi, **not** NACHO tokens. The KAS→NACHO conversion happens
only at krc-20 payout-cycle time, at the prevailing market rate.

Rationale: KAS is the asset we're actually mining; NACHO price is a
moving target. Accruing in sompi means a miner's pending rebate
balance has a stable real-asset meaning between cycles. The legacy
pool used the same model — see `katpool_mainnet.miners_balance.nacho_rebate_kas`.

### 5. Tier is evaluated once per matured block, never per share

Per-share tier evaluation would mean one external lookup per share
(>100/s sustained) and would let a miner toggle tier mid-window in
ways that are hard to audit. Per-block evaluation:

- One external lookup per (wallet, matured block) pair.
- Cleaner audit story: the block's `share_allocation` rows record
  exactly which tier each wallet sat in for that reward.
- Miners gain or lose elite status against **future** blocks only.

### 6. Tier classifier behind a trait, real impl deferred to M3

`accountant::TierClassifier` is an `async_trait` with two
implementations:

- `StaticTierClassifier` (this milestone) — always returns a
  configured tier; for tests and as a safe fallback.
- `KasplexTierClassifier` (Phase 3 M3) — HTTP client against
  `https://krc721.kat.foundation/api/v1/krc721/mainnet/address/{addr}/NACHO`
  for NFT ownership, plus the KRC-20 `NACHO` balance endpoint, with
  a small in-process TTL cache (~5 min) so we don't hammer kasplex
  on every matured block.

On any classifier error the safe fallback is `Standard` (the lower
rebate tier). The pool never over-rebates from a transient upstream
failure.

### 7. Audit-trail schema migration deferred to M3

The plan adds three columns to `share_allocation`:
- `applied_topline_bps SMALLINT NOT NULL`
- `applied_rebate_bps  SMALLINT NOT NULL`
- `applied_tier        wallet_tier NOT NULL`

Each row will be self-describing — historical allocations remain
inspectable and reproducible across future config changes.

The migration lands in the M3 PR together with the code that
populates the columns, *not* in this M1 PR. Two reasons:

1. **Atomicity of intent.** A schema column nobody writes to is a
   readiness trap: somebody could later assume rows have non-NULL
   data when historical rows do not.
2. **Reversibility.** If M2 work changes our mind about the column
   shape (e.g. we decide to also record the `topline_bps`
   denominator or include a `block_id` cross-check), we haven't
   committed prematurely.

### 8. Wallet-tier postgres enum

The migration in M3 introduces `CREATE TYPE wallet_tier AS ENUM
('standard', 'elite')` as a first-class postgres enum, matched by
the Rust `WalletTier` enum via `sqlx::Type`. Defined now in
`accountant/src/config.rs` so M2 code can carry tier through
without a schema dependency.

## Consequences

### Positive

- Topline fee is a 5-second ops change (set env var, restart
  systemd unit), no code deploy required.
- Historical allocations are self-describing (after M3), so future
  topline changes don't invalidate audit queries.
- Float-config foot-gun eliminated.
- Tier evaluation is bounded: O(matured blocks × distinct active
  wallets) external lookups, not O(shares).

### Negative

- Tier classification depends on an external service
  (kasplex.kat.foundation). Mitigation: TTL cache; on error, fall
  back to `Standard` rather than block the cycle.
- Operator can set `KATPOOL_FEE_TOPLINE_BPS=0` and run the pool at
  no fee. Documented as supported but unusual.

### Out of scope for this ADR

- Mainnet vs. testnet topline overrides (currently a single
  topline applies regardless of network — both networks share the
  binary, but only mainnet sees real share traffic).
- Per-wallet topline overrides (none, by design — the topline is
  applied uniformly).
- The actual KAS→NACHO conversion at payout time (Phase 5 ADR).

## Validation

- `accountant::config::tests` exercises the parse and validation
  paths.
- `accountant/tests/consumer.rs` proves the M1 event-ingestion
  path doesn't depend on the tier model.
- The fee math is unit-tested in the M3 allocation crate (forward
  reference) using property tests over `(topline_bps, rebate_bps,
  gross)` triples that the balance equation holds for every
  combination in the allowed range.
