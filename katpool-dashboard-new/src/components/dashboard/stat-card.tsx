"use client";

import { type ReactNode } from "react";
import { motion } from "framer-motion";
import { Info } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CountUp } from "./count-up";
import { DeltaChip } from "./delta-chip";
import { Sparkline } from "./sparkline";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

interface StatCardProps {
  label: string;
  /** Numeric value to count up, or null while loading. */
  value: number | null;
  format: (v: number) => string;
  unit?: string;
  icon?: ReactNode;
  delta?: number | null;
  invertDelta?: boolean;
  spark?: number[];
  colorIndex?: number;
  hint?: string;
  loading?: boolean;
  className?: string;
}

/** A premium KPI tile: label, animated value, delta chip, and sparkline. */
export function StatCard({
  label,
  value,
  format,
  unit,
  icon,
  delta,
  invertDelta,
  spark,
  colorIndex = 0,
  hint,
  loading = false,
  className,
}: StatCardProps) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4, ease: "easeOut" }}
    >
      <Card className={cn("relative overflow-hidden p-5", className)}>
        <div className="flex items-start justify-between gap-2">
          <div className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
            {icon}
            <span>{label}</span>
            {hint ? (
              <Tooltip>
                <TooltipTrigger asChild>
                  <button aria-label={`About ${label}`} className="text-muted-foreground/70 hover:text-foreground">
                    <Info className="size-3.5" />
                  </button>
                </TooltipTrigger>
                <TooltipContent>{hint}</TooltipContent>
              </Tooltip>
            ) : null}
          </div>
          {delta !== undefined ? <DeltaChip value={delta} invert={invertDelta} /> : null}
        </div>

        <div className="mt-3 flex items-end gap-1.5">
          {loading || value == null ? (
            <Skeleton className="h-8 w-28" />
          ) : (
            <CountUp value={value} format={format} className="text-3xl font-semibold tracking-tight tnum" />
          )}
          {unit ? <span className="pb-1 text-sm text-muted-foreground">{unit}</span> : null}
        </div>

        {spark && spark.length > 1 ? (
          <div className="mt-3 -mb-1">
            <Sparkline data={spark} colorIndex={colorIndex} />
          </div>
        ) : null}
      </Card>
    </motion.div>
  );
}
