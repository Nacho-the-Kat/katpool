"use client";

import { useState } from "react";
import { ChevronLeft, ChevronRight, ExternalLink } from "lucide-react";
import { Panel } from "@/components/dashboard/panel";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { EmptyState, ErrorState, LoadingRows } from "@/components/dashboard/states";
import { useMinerPayouts } from "@/lib/api/hooks";
import { formatDateTime, formatKas, formatRelative } from "@/lib/format";
import { explorerTx } from "@/lib/explorer";
import type { PayoutStatus } from "@/lib/api/types";

const PAGE = 25;

const STATUS: Record<PayoutStatus, "default" | "secondary" | "success" | "warning" | "destructive" | "outline"> = {
  planned: "outline",
  submitted: "secondary",
  accepted: "secondary",
  confirmed: "success",
  failed: "destructive",
};

/** Per-miner payout history (keyset-paginated). */
export function MinerPayouts({ address }: { address: string }) {
  const [stack, setStack] = useState<number[]>([]);
  const before = stack[stack.length - 1];
  const { data, isLoading, isError, refetch } = useMinerPayouts(address, PAGE, before);
  const payouts = data?.payouts ?? [];

  return (
    <Panel
      title="Payout history"
      description="Your KAS and NACHO payouts, newest first"
      actions={
        <div className="flex items-center gap-1">
          <Button variant="outline" size="icon" aria-label="Previous page" disabled={stack.length === 0} onClick={() => setStack((s) => s.slice(0, -1))}>
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
          <LoadingRows rows={6} />
        </div>
      ) : payouts.length === 0 ? (
        <div className="p-5">
          <EmptyState title="No payouts yet" description="Payouts appear here once you reach the threshold and a cycle settles." />
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                <th className="px-5 py-3 font-medium">Asset</th>
                <th className="px-5 py-3 text-right font-medium">Amount</th>
                <th className="px-5 py-3 font-medium">Status</th>
                <th className="px-5 py-3 font-medium">Tx</th>
                <th className="px-5 py-3 text-right font-medium">When</th>
              </tr>
            </thead>
            <tbody>
              {payouts.map((p) => {
                const tx = p.tx_hash ?? p.krc20_reveal_hash ?? p.krc20_commit_hash;
                return (
                  <tr key={p.id} className="border-b border-border/60 transition-colors hover:bg-muted/40">
                    <td className="px-5 py-3">
                      <Badge variant={p.kind === "kas" ? "default" : "secondary"}>
                        {p.kind === "kas" ? "KAS" : "NACHO"}
                      </Badge>
                    </td>
                    <td className="px-5 py-3 text-right tnum">{formatKas(p.amount.kas)}</td>
                    <td className="px-5 py-3">
                      <Badge variant={STATUS[p.status]}>{p.status}</Badge>
                    </td>
                    <td className="px-5 py-3">
                      {tx ? (
                        <a
                          href={explorerTx(tx)}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="inline-flex items-center gap-1 font-mono text-xs text-primary hover:underline"
                        >
                          {tx.slice(0, 8)}… <ExternalLink className="size-3" />
                        </a>
                      ) : (
                        <span className="text-muted-foreground">—</span>
                      )}
                    </td>
                    <td
                      className="px-5 py-3 text-right text-muted-foreground"
                      title={formatDateTime(p.confirmed_at ?? p.planned_at)}
                    >
                      {formatRelative(p.confirmed_at ?? p.planned_at)}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  );
}
