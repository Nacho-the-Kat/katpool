# Runbook 22 — Mainnet cutover execution

The "hands on keyboard" sequence to move miners from the legacy pool to the new
Rust pool. Simple and **foolproof**: every step protects one invariant —
no miner loses an unpaid balance, the treasury is never spent by two pools at
once, and the whole thing is reversible by flipping DNS back.

The handoff is **DNS-driven**: miners reconnect to the new pool on their own as
the 60 s-TTL record propagates, so there is no "connection freeze" step — when
legacy stops, sessions drop and reconnect to the new edge.

## The invariants (why each step exists)

1. **No lost balances** → legacy pays out every pending KAS balance *before* it
   stops (the importer carries history + NACHO rebates, **not** pending KAS).
2. **No double-spend / contamination** → only one pool spends the treasury at a
   time: legacy stops *before* the new pool goes live on the treasury address.
3. **Correct key** → the treasury key controls the treasury address
   (`katpool treasury audit`, **verified ✓**).
4. **Bounded blast radius** → per-cycle spend caps set before payouts go live.
5. **Reversible** → legacy containers are stopped, never deleted; rollback is a
   DNS flip back + `docker compose start`.

## Pre-cutover gates (all must be true)

- [x] Canary soaked clean on the new pool (shares accepted end-to-end through
      the fly edge; observability green).
- [x] **Treasury key audit green** — `katpool treasury audit` confirms the key
      controls `kaspa:qz4j8mu…jxnp`.
- [x] fly anycast edge live + reachable (`kas.katpool.com:5555` TCP OK), origin
      nftables allowlist applied, `KATPOOL_STRATUM_PROXY_PROTOCOL=true`.
- [ ] **Importer dry-run reconcile passes** — `legacy-importer-rehearsal.sh`
      (Runbook 14) exits 0 with `reconcile_all_passed == true`.
- [ ] **Spend caps set** in `ops/env/mainnet.env`:
      `KATPOOL_PAYOUT_MAX_SOMPI_PER_CYCLE`, `KATPOOL_KRC20_MAX_NACHO_PER_CYCLE`.
- [ ] **Rollback dry-checked**: legacy containers intact; confirm the legacy
      stratum IP (`152.53.37.182`) is recorded to flip DNS back to.
- [ ] DNS TTL on the `.xyz` stratum records lowered to **60 s ≥ a few hours
      ahead** so the flip propagates fast.

## Cutover (one ~5-minute window)

1. **Snapshot (rollback safety).** `pg_dump` the legacy DB →
   `cutover-evidence/…pre_cutover_<ts>.sql.gz`; record treasury KAS/NACHO
   balances + the legacy block count/last hash.
2. **Flush legacy.** Trigger a final legacy KAS payout of all pending balances;
   confirm pending → 0. *(Invariant 1.)*
3. **Stop legacy.** `docker compose stop katpool-app go-app katpool-payment
   katpool-monitor katpool-backup` — **do not remove** (rollback). *(Invariant 2.)*
4. **Import + reconcile.** Set `LEGACY_DATABASE_URL` + `KATPOOL_DATABASE_URL`,
   run `./scripts/legacy-importer-rehearsal.sh --no-dry-run`.
   **Gate:** importer exit 0 **and** `reconcile_all_passed == true`, else abort.
5. **Promote the new pool.** In `ops/env/mainnet.env`: set
   `KATPOOL_POOL_ADDRESS` → the treasury address, `KATPOOL_TREASURY_CREDENTIAL`
   (key cred), and the spend caps; keep `*_PAYOUT_DRY_RUN=true` for now. Deploy
   (`scripts/deploy.sh --network mainnet`) and confirm `/ready`.
6. **Flip DNS** — point every `.xyz` stratum record (and `kas.katpool.com`) at
   the fly anycast IP **`137.66.3.144`** / **`2a09:8280:1::129:8e82:0`**. Miners
   reconnect over ~1–2 min. *(Invariant 5 is the reverse of this.)*
7. **Verify, then go live.** Shares accepted; coinbases land on the treasury;
   the public API + MiningPoolStats feed (`/api/pool/miningPoolStats`) serve
   from the new pool. Let one payout **dry-run** cycle log a clean plan, then
   flip `KATPOOL_PAYOUT_DRY_RUN=false` + `KATPOOL_KRC20_PAYOUT_DRY_RUN=false`
   and confirm the first live cycle settles on-chain.

## Rollback (any gate fails)

1. Flip the `.xyz` stratum DNS back to `152.53.37.182`.
2. `docker compose start` the stopped legacy containers (intact from step 3).
3. The legacy importer is append/idempotent — safe to re-run later. Record the
   abort + cause in `cutover-evidence/`.

## Note: treasury coinbase re-discovery (benign)

When the new pool adopts the treasury address (step 5) its maturity tracker
scans the treasury's coinbase UTXOs and will re-record legacy's historical ones.
These have DAA windows that **predate any new-pool share**, so they allocate to
**no** wallet and are retained by the pool — not double-paid (legacy already
paid those finders). The effect is log/DB noise, not a money risk. If undesired,
bound coinbase-reward discovery to post-cutover DAA before promoting.

---

See [`docs/cutover-plan.md`](../cutover-plan.md) for the full rationale, the
comms templates, and the optional dress rehearsal.
