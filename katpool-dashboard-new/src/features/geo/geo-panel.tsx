"use client";

import { useMemo } from "react";
import { Globe } from "lucide-react";
import { Panel } from "@/components/dashboard/panel";
import { HBarChart } from "@/components/charts/bar-chart";
import { EmptyState, ErrorState, LoadingRows } from "@/components/dashboard/states";
import { usePoolGeo } from "@/lib/api/hooks";
import { formatNumber } from "@/lib/format";
import { countryName, flagEmoji } from "@/lib/country";

const WINDOW = 24 * 60 * 60;
const TOP_N = 8;

/**
 * Pool-wide miner distribution by country (last 24h). Sourced from the
 * aggregate-only `/pool/geo` endpoint (MaxMind GeoLite2; ADR-0025).
 * Forward-only: fills in as miner sessions close after deploy.
 */
export function GeoPanel() {
  const { data, isLoading, isError, refetch } = usePoolGeo(WINDOW);

  const bars = useMemo(
    () =>
      (data?.entries ?? []).slice(0, TOP_N).map((e) => ({
        label: `${flagEmoji(e.country)}  ${countryName(e.country)}`,
        value: e.sessions,
      })),
    [data],
  );

  const countries = data?.entries.length ?? 0;
  const description = countries
    ? `Sessions across ${formatNumber(countries)} ${countries === 1 ? "country" : "countries"} (last 24h)`
    : "Miner countries (last 24h)";

  return (
    <Panel title="Global distribution" description={description}>
      {isError ? (
        <ErrorState onRetry={() => void refetch()} />
      ) : isLoading ? (
        <LoadingRows rows={5} />
      ) : bars.length === 0 ? (
        <EmptyState
          icon={<Globe className="size-7" />}
          title="Mapping miners"
          description="Country distribution is resolved as miner sessions close, so this fills in over time."
        />
      ) : (
        <HBarChart
          data={bars}
          valueFormatter={(v) => `${formatNumber(v)} ${v === 1 ? "session" : "sessions"}`}
          colorIndex={2}
          height={Math.max(220, bars.length * 44)}
        />
      )}
    </Panel>
  );
}
