"use client";

import { useEffect, useRef, useState } from "react";

interface CountUpProps {
  value: number;
  /** Maps the animated numeric value to a display string. */
  format: (v: number) => string;
  durationMs?: number;
  className?: string;
}

/**
 * Animate a number from its previous value to the next.
 * A frame is committed only when the formatted text actually changes.
 */
export function CountUp({ value, format, durationMs = 700, className }: CountUpProps) {
  const formatRef = useRef(format);
  formatRef.current = format;
  const shownRef = useRef(value);
  const fromRef = useRef(value);
  const rafRef = useRef<number | null>(null);
  const [text, setText] = useState(() => format(value));

  useEffect(() => {
    const paint = (n: number) => {
      shownRef.current = n;
      const next = formatRef.current(n);
      setText((prev) => (prev === next ? prev : next));
    };
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce || !Number.isFinite(value)) {
      paint(value);
      fromRef.current = value;
      return;
    }
    const from = fromRef.current;
    if (from === value) {
      paint(value);
      return;
    }
    const start = performance.now();
    const tick = (now: number) => {
      const p = Math.min((now - start) / durationMs, 1);
      const eased = 1 - (1 - p) ** 3;
      paint(from + (value - from) * eased);
      if (p < 1) {
        rafRef.current = requestAnimationFrame(tick);
      } else {
        fromRef.current = value;
      }
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => {
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current);
      fromRef.current = shownRef.current;
    };
  }, [value, durationMs]);

  return <span className={className}>{text}</span>;
}
