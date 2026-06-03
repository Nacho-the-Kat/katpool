"use client";

import { ArrowDownRight, ArrowUpRight, Minus } from "lucide-react";
import { formatPercent } from "@/lib/format";
import { cn } from "@/lib/utils";

/** A ▲/▼ percentage chip; neutral when the delta is null or ~0. */
export function DeltaChip({
  value,
  className,
  invert = false,
}: {
  value: number | null | undefined;
  className?: string;
  /** When true, down is good (e.g. reject rate). */
  invert?: boolean;
}) {
  if (value == null || !Number.isFinite(value) || Math.abs(value) < 0.05) {
    return (
      <span
        className={cn(
          "inline-flex items-center gap-0.5 rounded-full bg-muted px-1.5 py-0.5 text-xs font-medium text-muted-foreground tnum",
          className,
        )}
      >
        <Minus className="size-3" />
        0.0%
      </span>
    );
  }
  const up = value > 0;
  const good = invert ? !up : up;
  return (
    <span
      className={cn(
        "inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-xs font-medium tnum",
        good ? "bg-success/15 text-success" : "bg-destructive/15 text-destructive",
        className,
      )}
    >
      {up ? <ArrowUpRight className="size-3" /> : <ArrowDownRight className="size-3" />}
      {formatPercent(Math.abs(value))}
    </span>
  );
}
