"use client";

import { useMemo } from "react";
import { Info } from "lucide-react";
import { Panel } from "@/components/dashboard/panel";
import { DonutChart } from "@/components/charts/donut-chart";
import { EmptyState, ErrorState, LoadingRows } from "@/components/dashboard/states";
import { useFirmware } from "@/lib/api/hooks";
import { formatNumber } from "@/lib/format";

const WINDOW = 24 * 60 * 60;

/** Distribution of miner client software (forward-only: fills from deploy). */
export function FirmwarePanel() {
  const { data, isLoading, isError, refetch } = useFirmware(WINDOW);

  const items = useMemo(
    () =>
      (data?.entries ?? []).map((e) => ({
        name: e.app ?? "Unknown client",
        value: e.sessions,
      })),
    [data],
  );

  const totalSessions = useMemo(() => items.reduce((sum, i) => sum + i.value, 0), [items]);

  return (
    <Panel
      title="Miner software"
      description="Sessions by reported stratum user-agent (last 24h)"
    >
      {isError ? (
        <ErrorState onRetry={() => void refetch()} />
      ) : isLoading ? (
        <LoadingRows rows={5} />
      ) : items.length === 0 ? (
        <EmptyState
          icon={<Info className="size-7" />}
          title="Collecting since deploy"
          description="Firmware data is recorded as miner sessions close, so this fills in over time."
        />
      ) : (
        <DonutChart
          data={items}
          valueFormatter={(v) => `${formatNumber(v)} ${v === 1 ? "session" : "sessions"}`}
          centerValue={formatNumber(totalSessions)}
          centerLabel={totalSessions === 1 ? "session" : "sessions"}
          height={300}
        />
      )}
    </Panel>
  );
}
