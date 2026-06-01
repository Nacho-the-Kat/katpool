# Architectural Decision Records

Every architecturally-significant decision is captured as a Markdown
file in this directory in [MADR 4.0](https://adr.github.io/madr/)
format. The numbering is monotonic — once an ADR is accepted, its
number never moves. If a decision is later superseded, mark the
original's `status` as `superseded by [ADR-NNNN]` and write a new
ADR pointing back.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-rust-first.md) | Rust as primary language | accepted |
| [0002](0002-fork-rusty-kaspa-bridge.md) | Fork rusty-kaspa v1.1.0 bridge | accepted |
| [0003](0003-sops-only-treasury-custody.md) | sops + age for treasury secrets at rest | accepted |
| [0004](0004-self-host-observability.md) | Self-host Grafana LGTM on Railway | accepted |
| [0005](0005-netcup-vps-railway-edge.md) | Stay on NetCup VPS; Railway for edge | accepted |
| [0006](0006-postgres-17-pinned.md) | Pin PostgreSQL to 17 major | accepted |
| [0007](0007-pgbackrest-wal-archiving.md) | pgBackRest WAL streaming to Backblaze B2 | accepted |
| [0008](0008-hot-only-treasury-with-os-isolation.md) | Hot-only treasury with OS isolation | accepted |
| [0009](0009-automated-weekly-dr-validation.md) | Automated weekly DR validation | accepted |
| [0015](0015-krc20-inscription-envelope.md) | KRC-20 inscription envelope byte-compatible with production | accepted |

## When to write a new ADR

Any of the following is a strong signal:

- A change crosses a trust boundary
- A change introduces or removes a third-party service or
  dependency category
- A change alters the deployment shape (host, runtime, datastore)
- A change is hard to reverse (a database migration is hard to
  reverse, swapping `serde_json` for `simd-json` is not)
- A change involves money, custody, or cryptographic primitives
- A reviewer asks for an ADR (always honour this)

If a PR's description starts to sound like the body of an ADR, it
*is* an ADR. Promote it.

## Template

Copy [`template.md`](template.md) to `NNNN-short-title.md` where
NNNN is the next sequential number. Fill in the fields. Submit as
a PR; ADR PRs are reviewed for the rationale, not just the text.
