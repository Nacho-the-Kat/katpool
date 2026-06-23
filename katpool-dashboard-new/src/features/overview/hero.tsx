"use client";

import { useMemo, type ReactNode } from "react";
import { Reveal } from "@/components/dashboard/reveal";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CountUp } from "@/components/dashboard/count-up";
import { DeltaChip } from "@/components/dashboard/delta-chip";
import { Sparkline } from "@/components/dashboard/sparkline";
import { usePoolHashrateHistory, usePoolStats, useNetworkContext } from "@/lib/api/hooks";
import { totalBlocksFound } from "@/lib/api/types";
import { ECOSYSTEM } from "@/lib/ecosystem";
import { ExtLink } from "@/components/ext-link";
import { formatCompact, formatHashrate, formatNumber, formatUsd } from "@/lib/format";
import { resolveRange } from "@/lib/range";

function trendDelta(values: number[]): number | null {
  if (values.length < 2) return null;
  const first = values[0];
  const last = values[values.length - 1];
  if (first == null || last == null || first === 0) return null;
  return ((last - first) / first) * 100;
}

/** A compact metric cell within the hero's hairline-separated grid. */
function HeroStat({
  label,
  value,
  loading,
  extra,
}: {
  label: ReactNode;
  value: string | null;
  loading?: boolean;
  extra?: ReactNode;
}) {
  return (
    <div className="min-w-0 bg-card px-4 py-3.5">
      <p className="text-xs text-muted-foreground">{label}</p>
      <div className="mt-1.5 flex items-center gap-2">
        {loading || value == null ? (
          <Skeleton className="h-6 w-20" />
        ) : (
          <span className="truncate text-base font-semibold metric sm:text-lg">{value}</span>
        )}
        {extra}
      </div>
    </div>
  );
}

/**
 * The Overview headline: a display-scale, live pool-hashrate figure with
 * network-share context, a living sparkline, and a dense supporting grid.
 */
export function OverviewHero() {
  const stats = usePoolStats();
  const network = useNetworkContext();
  const day = useMemo(() => resolveRange("24h"), []);
  const history = usePoolHashrateHistory({ from: day.from, to: day.to, bucket: day.bucket });

  const hashSpark = useMemo(
    () => (history.data?.points ?? []).map((p) => p.hashrate_hs),
    [history.data],
  );
  const hashDelta = useMemo(() => trendDelta(hashSpark), [hashSpark]);

  // Headline matches the sparkline: use the latest point of the same 24h
  // history series (a bucket-averaged value) rather than the noisy
  // short-window stats estimate, so the big number and the chart agree and it
  // stops jumping. Falls back to the stats estimate before history loads.
  const poolHs =
    hashSpark.length > 0
      ? (hashSpark[hashSpark.length - 1] ?? null)
      : (stats.data?.hashrate_hs ?? null);
  const netHs = network.data?.network_hashrate_hs ?? 0;
  const netShare = poolHs != null && netHs > 0 ? (poolHs / netHs) * 100 : null;
  // Only surface a network share when it's physically plausible. A pool cannot
  // exceed the network it mines; an implausible (>100%) ratio means the network
  // context and pool estimate are momentarily out of step (e.g. a lagging
  // testnet difficulty), so we fall back to the descriptive line rather than
  // print an alarming ">100%".
  const shareLabel =
    netShare == null || netShare > 100
      ? null
      : netShare > 0 && netShare < 0.001
        ? "<0.001%"
        : `${netShare.toFixed(netShare < 1 ? 3 : 2)}%`;

  const loading = stats.isLoading;
  const netLoading = network.isLoading;

  return (
    <Reveal>
      <Card className="relative overflow-hidden">
        {/* Ambient brand wash */}
        <div className="pointer-events-none absolute inset-0 app-aurora opacity-70" />
        <div className="pointer-events-none absolute -right-28 -top-28 size-80 rounded-full bg-primary/10 blur-3xl" />

        <div className="relative grid gap-x-8 gap-y-6 p-6 sm:p-8 lg:grid-cols-12">
          {/* Headline */}
          <div className="flex flex-col justify-center lg:col-span-5">
            <div className="flex items-center gap-2 text-[0.6875rem] font-medium uppercase tracking-[0.14em] text-muted-foreground">
              <span className="size-2 rounded-full bg-success live-dot" />
              Pool hashrate · Live
            </div>

            <div className="mt-3 flex flex-wrap items-end gap-x-3 gap-y-2">
              {loading || poolHs == null ? (
                <Skeleton className="h-14 w-56" />
              ) : (
                <CountUp
                  value={poolHs}
                  format={(v) => formatHashrate(v)}
                  className="text-grad text-[2.75rem] font-semibold leading-none metric sm:text-[3.5rem]"
                />
              )}
              {hashDelta != null ? <DeltaChip value={hashDelta} className="mb-1.5" /> : null}
            </div>

            <p className="mt-3 text-sm text-muted-foreground">
              {shareLabel ? (
                <>
                  <span className="font-semibold text-foreground">{shareLabel}</span> of the total{" "}
                  <ExtLink href={ECOSYSTEM.kaspa}>Kaspa</ExtLink> network hashrate
                </>
              ) : (
                "Estimated from accepted share difficulty over the last 24 hours"
              )}
            </p>

            {hashSpark.length > 1 ? (
              <div className="mt-5 -mb-1" aria-hidden>
                <Sparkline data={hashSpark} colorIndex={0} height={56} />
              </div>
            ) : null}
          </div>

          {/* Supporting metrics — hairline-separated data grid */}
          <div className="lg:col-span-7">
            <div className="grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-border bg-border sm:grid-cols-3">
              <HeroStat
                label="Active miners"
                value={stats.data ? formatNumber(stats.data.miners_active) : null}
                loading={loading}
              />
              <HeroStat
                label="Active workers"
                value={stats.data ? formatNumber(stats.data.workers_active) : null}
                loading={loading}
              />
              <HeroStat
                label="Accepted shares"
                value={stats.data ? formatCompact(stats.data.accepted_shares) : null}
                loading={loading}
              />
              <HeroStat
                label="Blocks found"
                value={stats.data ? formatCompact(totalBlocksFound(stats.data.blocks)) : null}
                loading={loading}
              />
              <HeroStat
                label="Network hashrate"
                value={network.data ? formatHashrate(network.data.network_hashrate_hs) : null}
                loading={netLoading}
              />
              <HeroStat
                label={
                  <>
                    <ExtLink href={ECOSYSTEM.kaspa}>KAS</ExtLink> price
                  </>
                }
                value={network.data ? formatUsd(network.data.prices.kas_usd) : null}
                loading={netLoading}
                extra={network.data ? <DeltaChip value={network.data.prices.kas_change_24h} /> : null}
              />
            </div>
          </div>
        </div>
      </Card>
    </Reveal>
  );
}
