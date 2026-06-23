"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import type { MiningPoolStats } from "@/lib/pool-stats";
import { poolApiBase } from "@/lib/pool-stats";

const POLL_MS = 10_000;

export function usePoolStats(initial: MiningPoolStats | null) {
  const [stats, setStats] = useState<MiningPoolStats | null>(initial);
  const [syncing, setSyncing] = useState(!initial);
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const res = await fetch(`${poolApiBase()}/api/pool/miningPoolStats`, {
        cache: "no-store",
      });
      if (!res.ok) return;
      const next = (await res.json()) as MiningPoolStats;
      if (mounted.current) {
        setStats(next);
        setSyncing(false);
      }
    } catch {
      /* keep last good snapshot */
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const id = window.setInterval(refresh, POLL_MS);
    return () => {
      mounted.current = false;
      window.clearInterval(id);
    };
  }, [refresh]);

  return { stats, syncing, refresh };
}
