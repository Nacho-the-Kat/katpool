"use client";

import { useMemo, useState } from "react";
import { Panel } from "@/components/dashboard/panel";
import { RangeToggle } from "@/components/dashboard/range-toggle";
import { TimeSeriesChart } from "@/components/charts/time-series-chart";
import { ChartSkeleton, EmptyState, ErrorState } from "@/components/dashboard/states";
import { useActiveMinersHistory } from "@/lib/api/hooks";
import { formatNumber } from "@/lib/format";
import { MINERS_SERIES_RANGE_KEYS, type RangeKey } from "@/lib/range";

/** Active distinct miners over time. */
export function MinersPanel() {
  // Default to 24h (same as hashrate) — longer COUNT DISTINCT scans are
  // heavier; 7d+ needs the covering-index INCLUDE (wallet_id) to stay under
  // the API hard timeout.
  const [range, setRange] = useState<RangeKey>("24h");
  const { data, isLoading, isError, refetch } = useActiveMinersHistory(range);

  const series = useMemo(
    () => [
      {
        name: "Active miners",
        colorIndex: 2,
        points: (data?.points ?? []).map((p) => ({ t: p.bucket_start, v: p.miners })),
      },
    ],
    [data],
  );

  return (
    <Panel
      title="Active miners"
      description="Distinct wallets submitting shares per bucket"
      actions={
        <RangeToggle value={range} onChange={setRange} options={MINERS_SERIES_RANGE_KEYS} />
      }
    >
      {isError ? (
        <ErrorState onRetry={() => void refetch()} />
      ) : isLoading ? (
        <ChartSkeleton height={320} />
      ) : (series[0]?.points?.length ?? 0) === 0 ? (
        <EmptyState
          title="No data in this range"
          description="Try a wider range, or check back once the pool has more history."
        />
      ) : (
        <TimeSeriesChart series={series} valueFormatter={(v) => formatNumber(v)} height={320} />
      )}
    </Panel>
  );
}
