"use client";

import { useMemo } from "react";
import { Panel } from "@/components/dashboard/panel";
import { ErrorState, LoadingRows } from "@/components/dashboard/states";
import { DeltaChip } from "@/components/dashboard/delta-chip";
import { useNetworkContext } from "@/lib/api/hooks";
import {
  formatCompact,
  formatDuration,
  formatHashrate,
  formatUsd,
} from "@/lib/format";

function Stat({ label, value, extra }: { label: string; value: string; extra?: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-border bg-muted/20 p-3">
      <p className="text-xs text-muted-foreground">{label}</p>
      <div className="mt-1 flex items-center gap-2">
        <p className="text-lg font-semibold tnum">{value}</p>
        {extra}
      </div>
    </div>
  );
}

/** Kaspa network + market context (BFF-aggregated; degrades gracefully). */
export function NetworkPanel() {
  const { data, isLoading, isError, refetch } = useNetworkContext();

  const halvingIn = useMemo(() => {
    if (!data?.next_halving) return null;
    const secs = data.next_halving.timestamp - Date.now() / 1000;
    return secs > 0 ? formatDuration(secs) : "imminent";
  }, [data]);

  return (
    <Panel title="Kaspa network" description="Live network & market context">
      {isError ? (
        <ErrorState onRetry={() => void refetch()} />
      ) : isLoading || !data ? (
        <LoadingRows rows={4} />
      ) : (
        <div className="grid grid-cols-2 gap-3">
          <Stat label="Network hashrate" value={formatHashrate(data.network_hashrate_hs)} />
          <Stat label="Difficulty" value={formatCompact(data.difficulty)} />
          <Stat
            label="KAS price"
            value={formatUsd(data.prices.kas_usd)}
            extra={<DeltaChip value={data.prices.kas_change_24h} />}
          />
          <Stat
            label="NACHO price"
            value={formatUsd(data.prices.nacho_usd)}
            extra={<DeltaChip value={data.prices.nacho_change_24h} />}
          />
          <Stat label="KAS market cap" value={formatUsd(data.prices.kas_market_cap_usd)} />
          <Stat label="Block reward" value={`${data.block_reward_kas.toFixed(2)} KAS`} />
          <Stat label="Circulating supply" value={`${formatCompact(data.circulating_supply_kas)} KAS`} />
          <Stat
            label="Next halving"
            value={halvingIn ?? "—"}
            extra={
              data.next_halving ? (
                <span className="text-xs text-muted-foreground">→ {data.next_halving.reward_kas.toFixed(2)}</span>
              ) : undefined
            }
          />
          {data.degraded.length > 0 ? (
            <p className="col-span-2 text-xs text-warning">
              Some sources are temporarily unavailable ({data.degraded.join(", ")}).
            </p>
          ) : null}
        </div>
      )}
    </Panel>
  );
}
