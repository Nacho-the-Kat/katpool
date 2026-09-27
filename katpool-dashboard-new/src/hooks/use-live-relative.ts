"use client";

import { useEffect, useState } from "react";
import { subscribeNow } from "@/hooks/use-now";
import { formatRelative } from "@/lib/format";

/** Relative time string. Prefer `<LiveRelative>` in lists so parents do not tick. */
export function useLiveRelative(iso: string | number | Date | null | undefined): string {
  const [label, setLabel] = useState(() => (iso == null ? "—" : formatRelative(iso, Date.now())));

  useEffect(() => {
    if (iso == null) return;
    const paint = (now: number) => {
      const next = formatRelative(iso, now);
      setLabel((prev) => (prev === next ? prev : next));
    };
    paint(Date.now());
    return subscribeNow(paint);
  }, [iso]);

  return iso == null ? "—" : label;
}
