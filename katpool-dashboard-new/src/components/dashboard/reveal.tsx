"use client";

import { type ReactNode, type CSSProperties } from "react";
import { cn } from "@/lib/utils";

interface RevealProps {
  children: ReactNode;
  className?: string;
  /** Stagger position; adds a small per-item delay. */
  index?: number;
  /** Unused. Kept so existing call sites stay source-compatible. */
  y?: number;
  /** Explicit delay in seconds (overrides index-based delay when set). */
  delay?: number;
}

/**
 * One compositor-only entrance. Reduced motion is handled in CSS so the
 * first paint does not wait on a motion runtime.
 */
export function Reveal({ children, className, index = 0, delay }: RevealProps) {
  const seconds = delay ?? Math.min(index * 0.05, 0.2);
  const style: CSSProperties | undefined =
    seconds > 0 ? { animationDelay: `${seconds}s` } : undefined;

  return (
    <div className={cn("reveal-in", className)} style={style}>
      {children}
    </div>
  );
}
