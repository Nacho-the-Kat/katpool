"use client";

import { useEffect, useRef } from "react";
import { subscribeNow } from "@/hooks/use-now";
import { formatRelative } from "@/lib/format";

/** Relative timestamp. Updates the text node directly so a table of ages does not re-render. */
export function LiveRelative({
  at,
  className,
  title,
}: {
  at: string | number | Date | null | undefined;
  className?: string;
  title?: string;
}) {
  const ref = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el || at == null) return;
    const paint = (now: number) => {
      el.textContent = formatRelative(at, now);
    };
    paint(Date.now());
    return subscribeNow(paint);
  }, [at]);

  return (
    <span ref={ref} className={className} title={title} suppressHydrationWarning>
      {at == null ? "—" : formatRelative(at, Date.now())}
    </span>
  );
}
