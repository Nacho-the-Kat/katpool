"use client";

import { useNetworkContext } from "@/lib/api/hooks";
import { formatUsd } from "@/lib/format";
import { DeltaChip } from "@/components/dashboard/delta-chip";
import { Skeleton } from "@/components/ui/skeleton";

/** Live KAS price + 24h change for the top bar. */
export function PriceTicker() {
  const { data, isLoading } = useNetworkContext();
  const kas = data?.prices.kas_usd ?? null;

  if (isLoading) return <Skeleton className="h-7 w-28 rounded-full" />;
  if (kas == null) return null;

  return (
    <div className="hidden items-center gap-2 rounded-full border border-border bg-muted/30 px-3 py-1 sm:flex">
      <span className="text-xs font-medium text-muted-foreground">KAS</span>
      <span className="text-sm font-semibold tnum">{formatUsd(kas)}</span>
      <DeltaChip value={data?.prices.kas_change_24h} />
    </div>
  );
}
