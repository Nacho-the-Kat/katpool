"use client";

import { AnimatePresence, motion } from "framer-motion";
import { useMemo } from "react";
import { useNow } from "@/hooks/use-now";
import { blockAgeParts, secondsSinceBlock } from "@/lib/pool-stats";
import { cn } from "@/lib/utils";

function Digit({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-col items-center gap-0.5">
      <div className="relative overflow-hidden rounded-lg border border-primary/25 bg-primary/8 px-2 py-1.5 shadow-[inset_0_1px_0_oklch(1_0_0/6%)] sm:px-2.5 sm:py-2">
        <AnimatePresence mode="popLayout" initial={false}>
          <motion.span
            key={value}
            initial={{ y: 14, opacity: 0, filter: "blur(4px)" }}
            animate={{ y: 0, opacity: 1, filter: "blur(0px)" }}
            exit={{ y: -14, opacity: 0, filter: "blur(4px)" }}
            transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
            className="metric block min-w-[1.35rem] text-center text-xl font-semibold leading-none sm:min-w-[1.6rem] sm:text-2xl"
          >
            {value}
          </motion.span>
        </AnimatePresence>
      </div>
      <span className="text-[9px] uppercase tracking-[0.14em] text-muted-foreground">{label}</span>
    </div>
  );
}

function Sep() {
  return (
    <span className="metric mb-4 self-center text-xl font-semibold text-primary/70 sm:text-2xl">:</span>
  );
}

export function BlockAgeClock({ lastBlockTime, className }: { lastBlockTime: string; className?: string }) {
  const now = useNow(1000);
  const sec = useMemo(() => secondsSinceBlock(lastBlockTime, now), [lastBlockTime, now]);
  const parts = blockAgeParts(sec);
  const fresh = sec < 90;

  return (
    <div className={cn("flex flex-col gap-2", className)}>
      <div className="flex items-end gap-1 sm:gap-1.5">
        {parts.showHours && (
          <>
            <Digit value={parts.hours} label="hr" />
            <Sep />
          </>
        )}
        <Digit value={parts.minutes} label="min" />
        <Sep />
        <Digit value={parts.seconds} label="sec" />
      </div>
      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <span
          className={cn(
            "size-1.5 rounded-full",
            fresh ? "live-dot bg-success" : "bg-muted-foreground/50",
          )}
        />
        {fresh ? "Fresh block — pool is hot" : "since last block found"}
      </p>
    </div>
  );
}
