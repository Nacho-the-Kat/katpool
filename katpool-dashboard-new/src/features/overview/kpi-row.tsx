"use client";

import { useMemo } from "react";
import { Activity, Cpu, Gauge, Layers, Network, Users } from "lucide-react";
import { StatCard } from "@/components/dashboard/stat-card";
import { usePoolHashrateHistory, usePoolStats, useNetworkContext } from "@/lib/api/hooks";
import { formatHashrate, formatNumber } from "@/lib/format";
import { resolveRange } from "@/lib/range";

/** Compute a percentage delta between the first and last samples. */
function trendDelta(values: number[]): number | null {
  if (values.length < 2) return null;
  const first = values[0];
  const last = values[values.length - 1];
  if (first == null || last == null || first === 0) return null;
  return ((last - first) / first) * 100;
}

/** The headline KPI grid for the overview page. */
export function KpiRow() {
  const stats = usePoolStats();
  const network = useNetworkContext();
  const day = useMemo(() => resolveRange("24h"), []);
  const history = usePoolHashrateHistory({ from: day.from, to: day.to, bucket: day.bucket });

  const hashSpark = useMemo(
    () => (history.data?.points ?? []).map((p) => p.hashrate_hs),
    [history.data],
  );
  const hashDelta = useMemo(() => trendDelta(hashSpark), [hashSpark]);

  const poolHs = stats.data?.hashrate_hs ?? null;
  const netHs = network.data?.network_hashrate_hs ?? 0;
  const netShare = poolHs != null && netHs > 0 ? (poolHs / netHs) * 100 : null;

  const loading = stats.isLoading;

  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
      <StatCard
        label="Pool hashrate"
        icon={<Gauge className="size-4" />}
        value={poolHs}
        format={(v) => formatHashrate(v)}
        delta={hashDelta}
        spark={hashSpark}
        colorIndex={0}
        loading={loading}
        hint="Estimated from accepted share difficulty over the recent window."
      />
      <StatCard
        label="Active miners"
        icon={<Users className="size-4" />}
        value={stats.data?.miners_active ?? null}
        format={(v) => formatNumber(Math.round(v))}
        loading={loading}
        hint="Distinct wallet addresses submitting accepted shares in the window."
      />
      <StatCard
        label="Active workers"
        icon={<Cpu className="size-4" />}
        value={stats.data?.workers_active ?? null}
        format={(v) => formatNumber(Math.round(v))}
        loading={loading}
        hint="Distinct worker rigs across all active miners."
      />
      <StatCard
        label="Accepted shares"
        icon={<Activity className="size-4" />}
        value={stats.data?.accepted_shares ?? null}
        format={(v) => formatNumber(Math.round(v))}
        loading={loading}
        hint="Total accepted shares in the recent window."
      />
      <StatCard
        label="Network share"
        icon={<Network className="size-4" />}
        value={netShare}
        // On small networks (e.g. testnet) the pool can exceed the network
        // estimate; never render an absurd multi-thousand-percent figure.
        format={(v) => (v > 100 ? ">100%" : `${v.toFixed(v < 1 ? 3 : 2)}%`)}
        loading={loading || network.isLoading}
        hint="Pool hashrate as a fraction of total Kaspa network hashrate."
      />
      <StatCard
        label="Blocks matured"
        icon={<Layers className="size-4" />}
        value={stats.data?.blocks.matured ?? null}
        format={(v) => formatNumber(Math.round(v))}
        loading={loading}
        hint="Coinbase-matured blocks found by the pool (lifetime)."
      />
    </div>
  );
}
