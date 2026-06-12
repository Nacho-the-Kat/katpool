# katpool fly.io anycast stratum edge

Thin per-region TCP forwarder that fronts the NetCup origin for a
zero-action mainnet cutover (see
[ADR-0022](../../../docs/decisions/0022-multiport-stratum-and-flyio-anycast-edge.md)
and
[`cutover-stratum-compatibility.md`](../../../docs/cutover-stratum-compatibility.md)).

## What it does

- One fly.io app, one **dedicated anycast IPv4**, deployed to the 7
  legacy regions. fly anycast routes each miner to the nearest healthy
  machine.
- Each of the 8 legacy stratum ports (`1111`–`8888`) is a fly TCP
  service with the `proxy_proto` (v2) handler. fly prepends a PROXY v2
  header carrying the real miner IP+port.
- HAProxy (`haproxy.cfg`) consumes that header (`accept-proxy`) and
  re-emits it to the origin (`send-proxy-v2`) on the **same** port, so
  the origin's per-port difficulty seed stays correct and the bridge
  recovers the real miner IP for anti-abuse + share attribution.

```
miner ──TCP:7777──▶ fly anycast (nearest region)
                      └─ proxy_proto v2 ─▶ HAProxy :7777 (accept-proxy)
                                              └─ send-proxy-v2 ─▶ origin:7777
```

## Regions (legacy parity)

| Hostname | Region | fly code |
|---|---|---|
| `na-west.katpool.com` | California | `sjc` |
| `na-east.katpool.com` | Virginia | `iad` |
| `eu.katpool.com` | Germany | `fra` |
| `ap.katpool.com` | Singapore | `sin` |
| `hkg.katpool.com` | Hong Kong | `hkg` |
| `sa.katpool.com` | Brazil | `gru` |
| `au.katpool.com` | Australia | `syd` |

`kas.katpool.com` (origin name) and all `*.katpool.com` mirrors also
resolve to the anycast IP — every miner connection arrives PROXY-fronted
(uniform path; ADR-0022).

## Deploy

```bash
cd ops/edge/flyio

# 1. Create the app (no deploy yet).
fly apps create katpool-edge

# 2. Point the forwarder at the origin (a name that resolves to the
#    NetCup origin's REAL IP — never the anycast/public name, or you loop).
fly secrets set KATPOOL_ORIGIN_HOST=kas-origin.katpool.com

# 3. Dedicated anycast IPv4 (raw TCP fails on fly's shared IPv4) + v6.
fly ips allocate-v4
fly ips allocate-v6

# 4. Stable per-region egress IPs for the origin allowlist (one per region).
for r in sjc iad fra sin hkg gru syd; do fly ips allocate-egress -r "$r"; done
fly ips list   # record the egress IPs -> origin nftables allowlist (below)

# 5. Deploy and spread across regions.
fly deploy
fly scale count 7 --region sjc,iad,fra,sin,hkg,gru,syd

# 6. Verify the anycast IP and per-region machines.
fly ips list
fly status
```

## Origin firewall (nftables) — REQUIRED

The origin must accept the stratum ports **only** from the fly egress
IPs, and the bridge must require PROXY v2 there
(`KATPOOL_STRATUM_PROXY_PROTOCOL=true`). Without this, anyone could open
the origin ports and spoof a PROXY header to forge a client IP.

The ruleset lives in [`nftables/katpool-stratum.nft`](nftables/katpool-stratum.nft)
(ships with RFC 5737/3849 documentation IPs as placeholders, so it is
syntactically valid but matches no real host). Fill in the real egress IPs and
apply it with the helper, which pulls them from `fly ips list`, validates with
`nft -c`, installs to `/etc/nftables.d/`, and loads them:

```bash
# Auto-collect egress IPs from `fly ips list` and apply (run on the origin):
sudo ops/edge/flyio/nftables/apply-origin-firewall.sh

# Or pass them explicitly (e.g. from step 4 above):
sudo ops/edge/flyio/nftables/apply-origin-firewall.sh 1.2.3.4 5.6.7.8 2a09:0:1::1

# Validate-only (no changes), or preview the rendered ruleset:
ops/edge/flyio/nftables/apply-origin-firewall.sh --check 1.2.3.4
ops/edge/flyio/nftables/apply-origin-firewall.sh --print 1.2.3.4
```

The ruleset touches **only** the stratum ports (chain policy is `accept`, so
SSH/API/kaspad are untouched) and fast-paths established connections so a
reload never severs an in-flight miner. Persist it across reboots by adding an
`include "/etc/nftables.d/katpool-stratum.nft"` to the host's nftables config,
and re-run the script after any `fly ips allocate-egress` change.

## DNS

Point every hostname's `A`/`AAAA` at the anycast IPs from `fly ips list`:

- `.xyz`: `kas`, `na-west`, `na-east`, `eu`, `ap`, `hkg`, `sa`, `au`.
- `.com`: the same set, mirrored (new in the rebuild; backward-compat).
- `kas-origin.katpool.com` ⇒ the NetCup origin's real IP (forwarder
  target only; not advertised to miners).

## Validation

1. `nc -vz <anycast-ip> 7777` from several geos — connect succeeds.
2. Point a tn10 hostname at a 2-region edge, mine with the Goldshell, and
   confirm on the origin that the logged client IP is the **ASIC's** IP
   (not a fly egress IP), and the `mining.set_difficulty` seed matches the
   port table.
3. Kill the nearest region; confirm anycast fails the miner over to the
   next region with reconnect only.

See ADR-0022 "Confirmation" for the full acceptance list.
