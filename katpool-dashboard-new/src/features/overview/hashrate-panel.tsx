"use client";

import { useMemo, useState } from "react";
import { Panel } from "@/components/dashboard/panel";
import { RangeToggle } from "@/components/dashboard/range-toggle";
import { TimeSeriesChart } from "@/components/charts/time-series-chart";
import { ChartSkeleton, EmptyState, ErrorState } from "@/components/dashboard/states";
import { usePoolHashrateHistory } from "@/lib/api/hooks";
import { formatHashrate } from "@/lib/format";
import { resolveRange, type RangeKey } from "@/lib/range";

/** Pool hashrate over time, with a range selector. */
export function HashratePanel() {
  const [range, setRange] = useState<RangeKey>("24h");
  const resolved = useMemo(() => resolveRange(range), [range]);
  const { data, isLoading, isError, refetch } = usePoolHashrateHistory({
    from: resolved.from,
    to: resolved.to,
    bucket: resolved.bucket,
  });

  const series = useMemo(
    () => [
      {
        name: "Pool hashrate",
        colorIndex: 0,
        points: (data?.points ?? []).map((p) => ({ t: p.bucket_start, v: p.hashrate_hs })),
      },
    ],
    [data],
  );

  return (
    <Panel
      title="Pool hashrate"
      description="Estimated from accepted share difficulty"
      actions={<RangeToggle value={range} onChange={setRange} />}
    >
      {isError ? (
        <ErrorState onRetry={() => void refetch()} />
      ) : isLoading ? (
        <ChartSkeleton height={320} />
      ) : series[0]?.points.length === 0 ? (
        <EmptyState
          title="No data in this range"
          description="Try a wider range, or check back once the pool has more history."
        />
      ) : (
        <TimeSeriesChart series={series} valueFormatter={(v) => formatHashrate(v)} showZoom height={320} />
      )}
    </Panel>
  );
}
