"use client";

import { Panel } from "@/components/dashboard/panel";
import { ErrorState, LoadingRows } from "@/components/dashboard/states";
import { DeltaChip } from "@/components/dashboard/delta-chip";
import { useNetworkContext } from "@/lib/api/hooks";
import { formatCompact, formatHashrate, formatUsd } from "@/lib/format";

function Stat({ label, value, extra }: { label: string; value: string; extra?: React.ReactNode }) {
  return (
    <div className="bg-card px-4 py-3">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="mt-1 truncate text-base font-semibold metric">{value}</p>
      {extra ? <div className="mt-1.5">{extra}</div> : null}
    </div>
  );
}

/** Kaspa network + market context (BFF-aggregated; degrades gracefully). */
export function NetworkPanel() {
  const { data, isLoading, isError, refetch } = useNetworkContext();

  return (
    <Panel title="Kaspa network" description="Live network & market context">
      {isError ? (
        <ErrorState onRetry={() => void refetch()} />
      ) : isLoading || !data ? (
        <LoadingRows rows={4} />
      ) : (
        <div className="grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-border bg-border">
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
          <Stat
            label="KAS market cap"
            value={
              data.prices.kas_market_cap_usd != null
                ? `$${formatCompact(data.prices.kas_market_cap_usd)}`
                : "—"
            }
          />
          <Stat
            label="Blue score"
            value={data.blue_score != null ? formatCompact(data.blue_score) : "—"}
          />
          {data.degraded.length > 0 ? (
            <p className="col-span-2 bg-card px-4 py-2 text-xs text-warning">
              Some sources are temporarily unavailable ({data.degraded.join(", ")}).
            </p>
          ) : null}
        </div>
      )}
    </Panel>
  );
}
