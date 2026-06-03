"use client";

import { usePoolStats } from "@/lib/api/hooks";
import { formatHashrate, formatNumber } from "@/lib/format";
import { Skeleton } from "@/components/ui/skeleton";

/** A compact live pool status block for the sidebar footer. */
export function PoolPulse() {
  const { data, isLoading, isError } = usePoolStats();

  return (
    <div className="mt-4 rounded-xl border border-border bg-muted/30 p-3">
      <div className="flex items-center gap-2">
        <span className="relative flex size-2">
          <span
            className={`absolute inline-flex size-full animate-ping rounded-full opacity-75 ${
              isError ? "bg-destructive" : "bg-success"
            }`}
          />
          <span
            className={`relative inline-flex size-2 rounded-full ${
              isError ? "bg-destructive" : "bg-success"
            }`}
          />
        </span>
        <span className="text-xs font-medium text-muted-foreground">
          {isError ? "Pool unreachable" : "Live"}
        </span>
      </div>
      <div className="mt-2 space-y-1">
        <div className="flex items-center justify-between text-xs">
          <span className="text-muted-foreground">Hashrate</span>
          {isLoading ? (
            <Skeleton className="h-3.5 w-16" />
          ) : (
            <span className="font-medium tnum">{formatHashrate(data?.hashrate_hs ?? 0)}</span>
          )}
        </div>
        <div className="flex items-center justify-between text-xs">
          <span className="text-muted-foreground">Miners</span>
          {isLoading ? (
            <Skeleton className="h-3.5 w-10" />
          ) : (
            <span className="font-medium tnum">{formatNumber(data?.miners_active ?? 0)}</span>
          )}
        </div>
      </div>
    </div>
  );
}
