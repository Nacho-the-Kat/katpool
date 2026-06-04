"use client";

import { useState } from "react";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { Panel } from "@/components/dashboard/panel";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { EmptyState, ErrorState, LoadingRows } from "@/components/dashboard/states";
import { LiveBadge } from "@/components/dashboard/live-badge";
import { CycleStatusBadge } from "./cycle-status-badge";
import { usePayoutCycles } from "@/lib/api/hooks";
import { useHighlightNew } from "@/lib/use-highlight-new";
import { formatDateTime, formatKas, formatNumber, formatRelative } from "@/lib/format";
import { cn } from "@/lib/utils";

const PAGE = 25;

/** Paginated table of payout cycles. */
export function CyclesTable() {
  const [stack, setStack] = useState<number[]>([]);
  const before = stack[stack.length - 1];
  const { data, isLoading, isError, refetch, dataUpdatedAt, isFetching } = usePayoutCycles(PAGE, before);

  // "Planned" cycles are an internal pre-broadcast bookkeeping state with no
  // distributed value yet; they only add noise to the public ledger, so hide
  // them and surface cycles from the moment they actually broadcast.
  const cycles = (data?.cycles ?? []).filter((c) => c.status !== "planned");
  const onFirstPage = stack.length === 0;
  const fresh = useHighlightNew(onFirstPage ? cycles.map((c) => c.id) : []);

  return (
    <Panel
      title="Payout cycles"
      description="KAS and NACHO distribution cycles, newest first"
      actions={
        <div className="flex items-center gap-2">
          <LiveBadge updatedAt={dataUpdatedAt} isFetching={isFetching} className="mr-1 hidden sm:inline-flex" />
          <Button
            variant="outline"
            size="icon"
            aria-label="Previous page"
            disabled={stack.length === 0}
            onClick={() => setStack((s) => s.slice(0, -1))}
          >
            <ChevronLeft className="size-4" />
          </Button>
          <Button
            variant="outline"
            size="icon"
            aria-label="Next page"
            disabled={data?.next_before == null}
            onClick={() => data?.next_before != null && setStack((s) => [...s, data.next_before!])}
          >
            <ChevronRight className="size-4" />
          </Button>
        </div>
      }
      bodyClassName="p-0"
    >
      {isError ? (
        <div className="p-5">
          <ErrorState onRetry={() => void refetch()} />
        </div>
      ) : isLoading ? (
        <div className="p-5">
          <LoadingRows rows={8} />
        </div>
      ) : cycles.length === 0 ? (
        <div className="p-5">
          <EmptyState title="No payout cycles yet" description="Distribution cycles appear here once the pool settles rewards." />
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full min-w-[680px] text-sm" aria-label="Payout cycles">
            <thead>
              <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                <th className="px-3 py-3 sm:px-5 font-medium">Cycle</th>
                <th className="px-3 py-3 sm:px-5 font-medium">Asset</th>
                <th className="px-3 py-3 sm:px-5 font-medium">Status</th>
                <th className="px-3 py-3 sm:px-5 text-right font-medium">Recipients</th>
                <th className="px-3 py-3 sm:px-5 text-right font-medium">Total</th>
                <th className="px-3 py-3 sm:px-5 text-right font-medium">When</th>
              </tr>
            </thead>
            <tbody>
              {cycles.map((c) => (
                <tr
                  key={c.id}
                  className={cn(
                    "border-b border-border/60 transition-colors hover:bg-muted/40",
                    fresh.has(c.id) && "row-flash",
                  )}
                >
                  <td className="px-3 py-3 sm:px-5 font-mono text-xs text-muted-foreground">#{c.id}</td>
                  <td className="px-3 py-3 sm:px-5">
                    <span className="inline-flex items-center gap-2">
                      <span
                        className={cn(
                          "size-1.5 rounded-full",
                          c.kind === "kas" ? "bg-primary" : "bg-secondary",
                        )}
                      />
                      <Badge variant={c.kind === "kas" ? "default" : "secondary"}>
                        {c.kind === "kas" ? "KAS" : "NACHO"}
                      </Badge>
                    </span>
                  </td>
                  <td className="px-3 py-3 sm:px-5">
                    <CycleStatusBadge status={c.status} />
                  </td>
                  <td className="px-3 py-3 sm:px-5 text-right tnum">
                    {c.total_recipients > 0 ? (
                      formatNumber(c.total_recipients)
                    ) : (
                      <span className="text-muted-foreground">—</span>
                    )}
                  </td>
                  <td className="px-3 py-3 sm:px-5 text-right">
                    <span className="block font-medium tnum">{formatKas(c.total.kas)}</span>
                    {c.kind === "nacho" ? (
                      <span className="block text-xs text-muted-foreground">KAS value</span>
                    ) : null}
                  </td>
                  <td
                    className="px-3 py-3 sm:px-5 text-right text-muted-foreground"
                    title={formatDateTime(c.settled_at ?? c.planned_at)}
                  >
                    {formatRelative(c.settled_at ?? c.planned_at)}
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
