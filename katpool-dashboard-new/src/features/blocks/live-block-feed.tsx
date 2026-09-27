"use client";

import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { Blocks, ExternalLink, Sparkles } from "lucide-react";
import { Panel } from "@/components/dashboard/panel";
import { EmptyState, ErrorState, LoadingRows } from "@/components/dashboard/states";
import { BlockStatusBadge } from "./block-status-badge";
import { useBlocks } from "@/lib/api/hooks";
import { LiveRelative } from "@/components/live-relative";
import { formatDateTime, formatNumber, truncateMiddle } from "@/lib/format";
import { explorerBlock } from "@/lib/explorer";
import { cn } from "@/lib/utils";

const FEED_SIZE = 7;
/** Don't fire the celebratory burst more than once per this window (ms). */
const CELEBRATION_COOLDOWN = 30_000;

interface Particle {
  x: number;
  y: number;
  size: number;
  color: string;
}

/** A short particle burst when the pool solves a block. CSS owns the motion. */
function BlockBurst({ trigger }: { trigger: number }) {
  const [particles, setParticles] = useState<Particle[] | null>(null);

  useEffect(() => {
    if (trigger === 0) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const palette = ["var(--primary)", "var(--secondary)", "var(--chart-3)"];
    setParticles(
      Array.from({ length: 12 }, (_, i) => {
        const angle = (Math.PI * 2 * i) / 12 + Math.random() * 0.4;
        const dist = 34 + Math.random() * 46;
        return {
          x: Math.cos(angle) * dist,
          y: Math.sin(angle) * dist,
          size: 4 + Math.random() * 4,
          color: palette[i % palette.length] ?? "var(--primary)",
        };
      }),
    );
    const id = setTimeout(() => setParticles(null), 1200);
    return () => clearTimeout(id);
  }, [trigger]);

  if (!particles) return null;
  return (
    <div className="pointer-events-none absolute left-1/2 top-9 z-20 -translate-x-1/2">
      {particles.map((p, i) => (
        <span
          key={i}
          className="absolute block rounded-full block-burst"
          style={
            {
              width: p.size,
              height: p.size,
              backgroundColor: p.color,
              "--bx": `${p.x}px`,
              "--by": `${p.y}px`,
            } as CSSProperties
          }
        />
      ))}
    </div>
  );
}

/**
 * A real-time feed of the latest pool blocks. New blocks flash in; solving a
 * block triggers a short, rate-limited burst.
 */
export function LiveBlockFeed() {
  const { data, isLoading, isError, refetch } = useBlocks(FEED_SIZE);
  const blocks = useMemo(() => data?.blocks ?? [], [data]);

  const lastIdRef = useRef<number | null>(null);
  const lastCelebRef = useRef(0);
  const [flash, setFlash] = useState(false);
  const [burst, setBurst] = useState(0);

  useEffect(() => {
    const top = blocks[0];
    if (!top) return;
    if (lastIdRef.current === null) {
      lastIdRef.current = top.id;
      return;
    }
    if (top.id !== lastIdRef.current) {
      lastIdRef.current = top.id;
      setFlash(true);
      const now = Date.now();
      if (now - lastCelebRef.current > CELEBRATION_COOLDOWN) {
        lastCelebRef.current = now;
        setBurst((b) => b + 1);
      }
      const t = setTimeout(() => setFlash(false), 1600);
      return () => clearTimeout(t);
    }
  }, [blocks]);

  return (
    <Panel
      eyebrow="Real-time"
      title="Live blocks"
      description={`The ${FEED_SIZE} most recent blocks the pool has found`}
      actions={
        <span className="inline-flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
          <span className="size-2 rounded-full bg-success live-dot" />
          Live
        </span>
      }
      bodyClassName="relative p-0"
    >
      <BlockBurst trigger={burst} />

      {flash ? (
        <div className="absolute right-5 top-3 z-10 inline-flex items-center gap-1.5 rounded-full border border-primary/40 bg-primary/15 px-2.5 py-1 text-xs font-semibold text-primary shadow-[var(--shadow-glow)] reveal-in">
          <Sparkles className="size-3.5" />
          Block found
        </div>
      ) : null}

      {isError ? (
        <div className="p-5">
          <ErrorState onRetry={() => void refetch()} />
        </div>
      ) : isLoading ? (
        <div className="p-5">
          <LoadingRows rows={FEED_SIZE} />
        </div>
      ) : blocks.length === 0 ? (
        <EmptyState
          icon={<Blocks className="size-6" />}
          title="Awaiting the first block"
          description="Every block the pool solves will stream in here the instant it's found."
        />
      ) : (
        <ul className="divide-y divide-border/50">
          {blocks.map((b, i) => (
            <li
              key={b.id}
              className={cn(
                "transition-colors hover:bg-muted/30",
                i === 0 && flash && "bg-primary/[0.06] row-flash",
              )}
            >
              <div className="flex items-center gap-3 px-5 py-3">
                <BlockStatusBadge status={b.status} />
                <a
                  href={explorerBlock(b.hash)}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="group inline-flex min-w-0 items-center gap-1.5 font-mono text-xs hover:text-primary"
                >
                  <span className="truncate">{truncateMiddle(b.hash, 10, 8)}</span>
                  <ExternalLink className="size-3 shrink-0 text-muted-foreground group-hover:text-primary" />
                </a>
                <div className="ml-auto flex shrink-0 items-center gap-4 text-xs">
                  <span className="hidden text-muted-foreground tabular-nums sm:inline">
                    DAA {formatNumber(b.daa_score)}
                  </span>
                  <LiveRelative
                    at={b.found_at}
                    className="text-muted-foreground"
                    title={formatDateTime(b.found_at)}
                  />
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}
