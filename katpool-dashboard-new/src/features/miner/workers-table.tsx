"use client";

import { Panel } from "@/components/dashboard/panel";
import { EmptyState, ErrorState, LoadingRows } from "@/components/dashboard/states";
import { useMinerWorkers } from "@/lib/api/hooks";
import { formatDateTime, formatHashrate, formatNumber, formatRelative } from "@/lib/format";

/** Per-worker breakdown for a miner. */
export function WorkersTable({ address }: { address: string }) {
  const { data, isLoading, isError, refetch } = useMinerWorkers(address);
  const workers = data?.workers ?? [];

  return (
    <Panel title="Workers" description="Per-rig activity in the recent window" bodyClassName="p-0">
      {isError ? (
        <div className="p-5">
          <ErrorState onRetry={() => void refetch()} />
        </div>
      ) : isLoading ? (
        <div className="p-5">
          <LoadingRows rows={5} />
        </div>
      ) : workers.length === 0 ? (
        <div className="p-5">
          <EmptyState title="No active workers" description="Workers appear here when they submit shares." />
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                <th className="px-5 py-3 font-medium">Worker</th>
                <th className="px-5 py-3 text-right font-medium">Hashrate</th>
                <th className="px-5 py-3 text-right font-medium">Shares</th>
                <th className="px-5 py-3 text-right font-medium">Last seen</th>
              </tr>
            </thead>
            <tbody>
              {workers.map((w) => (
                <tr key={w.name} className="border-b border-border/60 transition-colors hover:bg-muted/40">
                  <td className="px-5 py-3 font-medium">{w.name}</td>
                  <td className="px-5 py-3 text-right tnum">{formatHashrate(w.hashrate_hs)}</td>
                  <td className="px-5 py-3 text-right tnum text-muted-foreground">
                    {formatNumber(w.accepted_shares)}
                  </td>
                  <td className="px-5 py-3 text-right text-muted-foreground" title={formatDateTime(w.last_seen_at)}>
                    {formatRelative(w.last_seen_at)}
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
