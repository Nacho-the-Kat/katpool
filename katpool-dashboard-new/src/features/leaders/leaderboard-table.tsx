"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import { Panel } from "@/components/dashboard/panel";
import { RangeToggle } from "@/components/dashboard/range-toggle";
import { EmptyState, ErrorState, LoadingRows } from "@/components/dashboard/states";
import { AddressDisplay } from "@/components/dashboard/address-display";
import { useLeaderboard } from "@/lib/api/hooks";
import { formatHashrate, formatNumber } from "@/lib/format";
import { resolveRange, type RangeKey } from "@/lib/range";
import { cn } from "@/lib/utils";

const RANK_ACCENT = ["text-yellow-400", "text-zinc-300", "text-amber-600"];

/** Top miners by windowed hashrate, with a share-of-pool bar. */
export function LeaderboardTable({ limit = 50, compact = false }: { limit?: number; compact?: boolean }) {
  const [range, setRange] = useState<RangeKey>("24h");
  const resolved = useMemo(() => resolveRange(range), [range]);
  const { data, isLoading, isError, refetch } = useLeaderboard(resolved.windowSecs, limit);

  const entries = data?.entries ?? [];

  return (
    <Panel
      title="Top miners"
      description="Ranked by estimated hashrate over the window"
      actions={
        compact ? (
          <Link href="/leaders" className="text-xs font-medium text-primary hover:underline">
            View all
          </Link>
        ) : (
          <RangeToggle value={range} onChange={setRange} options={["1h", "24h", "7d", "30d"]} />
        )
      }
      bodyClassName="p-0"
    >
      {isError ? (
        <div className="p-5">
          <ErrorState onRetry={() => void refetch()} />
        </div>
      ) : isLoading ? (
        <div className="p-5">
          <LoadingRows rows={compact ? 5 : 10} />
        </div>
      ) : entries.length === 0 ? (
        <div className="p-5">
          <EmptyState title="No active miners" description="The leaderboard fills as miners submit shares." />
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                <th className="px-5 py-3 font-medium">#</th>
                <th className="px-5 py-3 font-medium">Miner</th>
                <th className="px-5 py-3 text-right font-medium">Hashrate</th>
                {!compact && <th className="px-5 py-3 text-right font-medium">Shares</th>}
                <th className="px-5 py-3 text-right font-medium">Pool share</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr key={e.address} className="border-b border-border/60 transition-colors hover:bg-muted/40">
                  <td className={cn("px-5 py-3 font-semibold tnum", RANK_ACCENT[e.rank - 1] ?? "text-muted-foreground")}>
                    {e.rank}
                  </td>
                  <td className="px-5 py-3">
                    <Link href={`/miners/${encodeURIComponent(e.address)}`} className="hover:underline">
                      <AddressDisplay address={e.address} link={false} />
                    </Link>
                  </td>
                  <td className="px-5 py-3 text-right tnum font-medium">{formatHashrate(e.hashrate_hs)}</td>
                  {!compact && (
                    <td className="px-5 py-3 text-right tnum text-muted-foreground">
                      {formatNumber(e.accepted_shares)}
                    </td>
                  )}
                  <td className="px-5 py-3">
                    <div className="flex items-center justify-end gap-2">
                      <div className="hidden h-1.5 w-20 overflow-hidden rounded-full bg-muted sm:block">
                        <div
                          className="h-full rounded-full bg-primary"
                          style={{ width: `${Math.min(e.pool_share * 100, 100).toFixed(1)}%` }}
                        />
                      </div>
                      <span className="tnum text-xs text-muted-foreground">
                        {(e.pool_share * 100).toFixed(e.pool_share < 0.01 ? 2 : 1)}%
                      </span>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  );
}
