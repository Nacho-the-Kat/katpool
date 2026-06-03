"use client";

import { useState } from "react";
import { ChevronLeft, ChevronRight, ExternalLink } from "lucide-react";
import { Panel } from "@/components/dashboard/panel";
import { Button } from "@/components/ui/button";
import { EmptyState, ErrorState, LoadingRows } from "@/components/dashboard/states";
import { CopyButton } from "@/components/dashboard/copy-button";
import { BlockStatusBadge } from "./block-status-badge";
import { useBlocks } from "@/lib/api/hooks";
import { formatDateTime, formatKas, formatNumber, formatRelative, truncateMiddle } from "@/lib/format";
import { explorerBlock } from "@/lib/explorer";

const PAGE = 25;

/** Paginated table of recent pool blocks (keyset pagination). */
export function BlocksTable() {
  const [stack, setStack] = useState<number[]>([]);
  const before = stack[stack.length - 1];
  const { data, isLoading, isError, refetch } = useBlocks(PAGE, before);

  const blocks = data?.blocks ?? [];
  // The reward field is populated on mainnet but null on TN10; only show the
  // column when at least one block carries a value, so it never reads as a
  // dead, unfinished column.
  const showReward = blocks.some((b) => b.reward != null);

  return (
    <Panel
      title="Recent blocks"
      description="Blocks found by the pool, newest first"
      actions={
        <div className="flex items-center gap-1">
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
      ) : blocks.length === 0 ? (
        <div className="p-5">
          <EmptyState title="No blocks yet" description="Blocks will appear here as the pool finds them." />
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                <th className="px-5 py-3 font-medium">Block hash</th>
                <th className="px-5 py-3 font-medium">Status</th>
                <th className="px-5 py-3 text-right font-medium">DAA score</th>
                {showReward && <th className="px-5 py-3 text-right font-medium">Reward</th>}
                <th className="px-5 py-3 text-right font-medium">Found</th>
              </tr>
            </thead>
            <tbody>
              {blocks.map((b) => (
                <tr key={b.id} className="border-b border-border/60 transition-colors hover:bg-muted/40">
                  <td className="px-5 py-3">
                    <span className="inline-flex items-center gap-1.5 font-mono text-xs">
                      {truncateMiddle(b.hash, 12, 8)}
                      <CopyButton value={b.hash} label="Copy block hash" />
                      <a
                        href={explorerBlock(b.hash)}
                        target="_blank"
                        rel="noopener noreferrer"
                        aria-label="View block on explorer"
                        className="inline-flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground"
                      >
                        <ExternalLink className="size-3.5" />
                      </a>
                    </span>
                  </td>
                  <td className="px-5 py-3">
                    <BlockStatusBadge status={b.status} />
                  </td>
                  <td className="px-5 py-3 text-right tnum">{formatNumber(b.daa_score)}</td>
                  {showReward && (
                    <td className="px-5 py-3 text-right tnum">
                      {b.reward ? formatKas(b.reward.kas) : <span className="text-muted-foreground">—</span>}
                    </td>
                  )}
                  <td className="px-5 py-3 text-right text-muted-foreground" title={formatDateTime(b.found_at)}>
                    {formatRelative(b.found_at)}
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
