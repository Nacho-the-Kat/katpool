---
status: accepted
date: 2026-05-26
deciders: argonmining
---

# ADR-0014: Block maturity tracker architecture

## Context and Problem Statement

Phase 3 M3 landed the [`AllocationEngine`], which converts a
matured `block` row into per-wallet `share_allocation` rows. It
takes the block reward as an argument and assumes the caller has
already decided the block is matured.

Something needs to *be* that caller. It must:

1. Notice when kaspad has confirmed a `submitted_to_node` block
   as blue.
2. Notice when a `confirmed_blue` block reaches coinbase
   maturity (≥ `maturity_depth` blue blocks after it).
3. Notice when a `confirmed_blue` block is orphaned by a DAG
   re-org.
4. Extract the coinbase reward (sompi paid to the pool's mining
   address) from the matured block.
5. Hand off to [`AllocationEngine::allocate_matured_block`].

This ADR captures the architectural decisions for the
`MaturityTracker` that does all of the above.

## Decision

### 1. Polling, not subscription

Kaspa's gRPC API supports both poll and notification-stream
patterns. The tracker uses **polling** for three reasons:

- **Simpler restart semantics.** A polling loop has no in-flight
  notification queue to reconcile on restart. The state of the
  world is fully derivable from the DB's `block` table plus
  kaspad's current DAG.
- **Bounded kaspad load.** Polling at a known cadence gives the
  operator a predictable RPS against kaspad. A notification
  stream that suddenly fans out 1000 messages can knock down a
  shared-VPS kaspad.
- **Trivial back-pressure.** The tracker reads at most
  `cfg.batch_size` blocks per sweep — never overruns the engine.

Default cadence: 15s. Operator-tunable via `MaturityConfig`.
Tighter for sub-minute allocation latency, looser to reduce
kaspad load on a busy node.

### 2. kaspad access behind a trait

The tracker depends on a [`KaspadClient`] trait, not on
`kaspa-grpc-client` directly. The trait surface is exactly two
methods:

```text
get_virtual_blue_score() -> u64
get_block(hash)          -> Option<BlockInfo>
```

`BlockInfo` carries `{hash, blue_score, is_blue, daa_score,
coinbase_reward_sompi}` — every datum the state machine needs.

**Why a trait, not a direct dependency:**

- **Testability.** The tracker's state machine is the bulk of
  the code under review. Stubbing kaspad behind a trait lets
  the test suite cover every transition path deterministically
  against an in-memory fake (`FakeKaspad` in
  `accountant/tests/maturity_tracker.rs`) without standing up a
  real kaspad-tn10 instance. Eleven tests; zero network.
- **Phased delivery.** Phase 3 M3b (this PR) ships the state
  machine + stub. The real gRPC-backed `KaspadGrpcClient` impl
  lands in M3c so the kaspad-integration surface (reward
  extraction from coinbase tx, address-recognition policy,
  reconnect / timeout semantics, gRPC error mapping) gets its
  own focused review independent of the tracker logic.

### 3. State-machine reads, atomic writes

Each sweep:

1. Reads `virtual_blue_score` once.
2. Reads the active block set (`status IN ('submitted_to_node',
   'confirmed_blue')`) once.
3. For each block:
   - `get_block(hash)` against kaspad.
   - Decide the next state.
   - Apply the transition via a single repo call.

If the engine call (on a `matured` transition) errors mid-flight,
the engine's own transaction rolls back; the tracker logs +
counts but doesn't unwind anything else. The next sweep retries
the same block.

### 4. Window-size policy

Each matured block triggers a PROP allocation over a DAA window
ending at `block.daa_score`. The window's `daa_start` is
`block.daa_score − cfg.window_daa_span`. Default span: 600 DAA
scores.

**Why 600:**

- Post-Crescendo BPS is 10 → 600 DAA ≈ 60 seconds.
- Pre-Crescendo BPS is 1 → 600 DAA ≈ 10 minutes.

A one-minute post-Crescendo window aligns with the legacy
pool's PPLNS-N convention without locking in a specific
share-count constant. Operator-tunable via `MaturityConfig` —
mid-cutover the operator may want to widen briefly to capture
shares from a slow-to-confirm legacy migration.

The chosen window-size is **NOT** automatically tied to coinbase
maturity. They're independent parameters:

- `maturity_depth` controls *when* a block matures (a chain
  safety parameter set by Kaspa consensus).
- `window_daa_span` controls *which shares* contribute to that
  block's reward allocation (a fairness parameter set by the
  pool).

### 5. Reward extraction is the `KaspadClient` impl's concern

`BlockInfo::coinbase_reward_sompi` is a single `i64` — the
tracker treats it as opaque. The decision of "which coinbase
outputs belong to the pool" is policy that lives entirely inside
the `KaspadClient` implementation (Phase 3 M3c).

Two reasons:

- The address-recognition policy (single mining address?
  multiple? change addresses?) hasn't stabilised yet; isolating
  it in the gRPC layer lets us iterate without touching the
  tracker.
- The tests for the tracker assert against integer rewards.
  Splitting "compute the reward" from "what to do with the
  reward" gives both halves their own focused test surface.

### 6. Per-block error isolation, whole-sweep error fail-fast

- **Per-block errors** (kaspad transient failure on
  `get_block`, DB constraint violation on a single block) are
  logged + counted in `SweepStats.errors` and the sweep
  continues.
- **Whole-sweep errors** (kaspad transport down for
  `get_virtual_blue_score`, DB pool unavailable for
  `list_by_status`) abort the sweep with a `TrackerError`. The
  `run_loop` catches and logs but doesn't kill the loop — the
  next tick retries.

This split surfaces "the world is broken" as a noisy log signal
while keeping a single bad block from gating every other.

### 7. `tokio::sync::watch` for shutdown

The `run_loop` takes a `watch::Receiver<bool>` for shutdown.
Setting the channel to `true` from the parent task causes the
loop to exit cleanly at the next select. Tested explicitly in
`run_loop_exits_cleanly_on_shutdown_signal`.

`watch` is preferred over `oneshot` because the receiver outlives
multiple potential restarts of the tracker task, and over
`CancellationToken` (from `tokio-util`) because we already depend
on `tokio` and don't need the extra crate just for shutdown.

## Consequences

### Positive

- The tracker's full state machine has deterministic test
  coverage against the in-memory fake. Production failures will
  be in the kaspad-integration layer (M3c) where they can be
  diagnosed without disentangling state-machine logic.
- Operator controls (poll cadence, maturity depth, window span,
  batch size) are all on one config struct.
- Idempotent: a sweep that crashes mid-block simply re-runs
  next tick. The engine's own idempotency (matured = no-op)
  prevents double-allocation.

### Negative

- Two-phase delivery means M3b doesn't end-to-end exercise
  against a real kaspad. M3c will add a manual integration
  script (not CI) that hits testnet-10's kaspad-tn10 to
  validate the full chain.
- 15-second polling cadence means worst-case 15s additional
  allocation latency beyond what kaspad needs to confirm and
  mature. Acceptable: PROP allocation is not a real-time
  service from the miner's perspective, and the operator can
  lower the interval for testing.

### Out of scope

- **Real-time push from kaspad** (a long-lived notification
  stream).
- **Reward extraction policy** — what counts as a "pool
  output" in the coinbase transaction. Lands in M3c with the
  gRPC client.

## Re-evaluation triggers

- A real kaspad call goes through this trait that doesn't fit
  the two-method surface.
- Polling at 15s shows up as a hot signal on kaspad's load
  graphs.
- The window-span default needs to change after Phase 7 load
  testing.

[`AllocationEngine`]: ../../accountant/src/allocation.rs
[`AllocationEngine::allocate_matured_block`]: ../../accountant/src/allocation.rs
[`KaspadClient`]: ../../accountant/src/maturity.rs
