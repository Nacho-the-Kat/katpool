"use client";

import { useQuery, type UseQueryResult } from "@tanstack/react-query";
import { bffUrl, fetchBff } from "./client";
import type {
  ActiveMinersHistory,
  ActiveSessions,
  BalanceResponse,
  BlocksPage,
  CyclesPage,
  CycleDetailPage,
  FirmwareBreakdown,
  GeoBreakdown,
  PoolRejectsResponse,
  FullRebateResponse,
  HashrateHistory,
  LeaderboardResponse,
  MinerPayoutsPage,
  MinerProfile,
  NetworkContext,
  PoolStats,
  RejectsResponse,
  WorkersResponse,
} from "./types";
import { LIVE_HASHRATE_POLL_MS, LIVE_HASHRATE_WINDOW_SECS } from "../hashrate-live";
import { MAX_WINDOW_SECS, resolveRange, type RangeKey } from "../range";

/** Default live-refresh cadence for pool-wide widgets (ms). */
const LIVE_MS = 10_000;
const NETWORK_MS = 60_000;

function useBff<T>(
  key: readonly unknown[],
  url: string,
  refetchInterval: number | false = false,
  enabled = true,
): UseQueryResult<T, Error> {
  return useQuery<T, Error>({
    queryKey: key,
    queryFn: ({ signal }) => fetchBff<T>(url, signal),
    refetchInterval,
    refetchIntervalInBackground: true,
    refetchOnMount: "always",
    enabled,
  });
}

/**
 * Time-series history keyed by range preset. `from`/`to` are resolved inside
 * the queryFn (quantized) so each poll slides the window without thrashing
 * the React Query cache key on every render.
 */
function useHistoryRange<T>(
  key: readonly unknown[],
  buildUrl: (range: ReturnType<typeof resolveRange>) => string,
  range: RangeKey,
  refetchInterval: number | false = LIVE_MS,
  enabled = true,
): UseQueryResult<T, Error> {
  return useQuery<T, Error>({
    queryKey: [...key, range],
    queryFn: ({ signal }) => fetchBff<T>(buildUrl(resolveRange(range)), signal),
    refetchInterval,
    refetchIntervalInBackground: true,
    refetchOnMount: "always",
    enabled,
  });
}

export function usePoolStats(windowSecs?: number) {
  const interval =
    windowSecs === LIVE_HASHRATE_WINDOW_SECS ? LIVE_HASHRATE_POLL_MS : LIVE_MS;
  return useBff<PoolStats>(
    ["pool", "stats", windowSecs ?? null],
    bffUrl("/api/v1/pool/stats", { window: windowSecs }),
    interval,
  );
}

/** Live headline hashrate: 5-minute sliding window, 5-second poll. */
export function usePoolLiveStats() {
  return usePoolStats(LIVE_HASHRATE_WINDOW_SECS);
}

export function usePoolHashrateHistory(range: RangeKey) {
  return useHistoryRange<HashrateHistory>(
    ["pool", "hashrate/history"],
    (r) => bffUrl("/api/v1/pool/hashrate/history", { from: r.from, to: r.to, bucket: r.bucket }),
    range,
  );
}

export function useActiveMinersHistory(range: RangeKey) {
  return useHistoryRange<ActiveMinersHistory>(
    ["pool", "miners/history"],
    (r) => bffUrl("/api/v1/pool/miners/history", { from: r.from, to: r.to, bucket: r.bucket }),
    range,
  );
}

export function useLeaderboard(windowSecs?: number, limit?: number) {
  // Clamp to the API's MAX_WINDOW so a caller can't request 7d and silently
  // receive 24h data (the server caps without error).
  const capped =
    windowSecs == null ? undefined : Math.min(windowSecs, MAX_WINDOW_SECS);
  return useBff<LeaderboardResponse>(
    ["pool", "leaderboard", capped ?? null, limit ?? null],
    bffUrl("/api/v1/pool/leaderboard", { window: capped, limit }),
    LIVE_MS,
  );
}

export function useFirmware(windowSecs?: number) {
  return useBff<FirmwareBreakdown>(
    ["pool", "firmware", windowSecs ?? null],
    bffUrl("/api/v1/pool/firmware", { window: windowSecs }),
    LIVE_MS,
  );
}

export function usePoolRejects(windowSecs?: number) {
  return useBff<PoolRejectsResponse>(
    ["pool", "rejects", windowSecs ?? null],
    bffUrl("/api/v1/pool/rejects", { window: windowSecs }),
    LIVE_MS,
  );
}

export function usePoolGeo(windowSecs?: number) {
  return useBff<GeoBreakdown>(
    ["pool", "geo", windowSecs ?? null],
    bffUrl("/api/v1/pool/geo", { window: windowSecs }),
    LIVE_MS,
  );
}

export function useActiveSessions() {
  return useBff<ActiveSessions>(
    ["pool", "active-sessions"],
    bffUrl("/api/v1/pool/active-sessions"),
    LIVE_MS,
  );
}

export function useBlocks(limit?: number, before?: number) {
  return useBff<BlocksPage>(
    ["pool", "blocks", limit ?? null, before ?? null],
    bffUrl("/api/v1/pool/blocks", { limit, before }),
    LIVE_MS,
  );
}

export function usePayoutCycles(limit?: number, before?: number) {
  return useBff<CyclesPage>(
    ["pool", "payouts", limit ?? null, before ?? null],
    bffUrl("/api/v1/pool/payouts", { limit, before }),
    LIVE_MS,
  );
}

export function usePayoutCycle(cycleId: number | null) {
  return useBff<CycleDetailPage>(
    ["pool", "payouts", "detail", cycleId],
    bffUrl(`/api/v1/pool/payouts/${cycleId}`),
    LIVE_MS,
    cycleId != null,
  );
}

export function useNetworkContext() {
  return useBff<NetworkContext>(["network"], "/api/network", NETWORK_MS);
}

// ---- per-miner -------------------------------------------------------

export function useMinerProfile(address: string, enabled = true) {
  return useBff<MinerProfile>(
    ["miner", address, "profile"],
    bffUrl(`/api/v1/miners/${encodeURIComponent(address)}`),
    LIVE_MS,
    enabled,
  );
}

export function useMinerWorkers(address: string, enabled = true) {
  return useBff<WorkersResponse>(
    ["miner", address, "workers"],
    bffUrl(`/api/v1/miners/${encodeURIComponent(address)}/workers`),
    LIVE_MS,
    enabled,
  );
}

export function useMinerHashrateHistory(address: string, range: RangeKey, enabled = true) {
  return useHistoryRange<HashrateHistory>(
    ["miner", address, "hashrate/history"],
    (r) =>
      bffUrl(`/api/v1/miners/${encodeURIComponent(address)}/hashrate/history`, {
        from: r.from,
        to: r.to,
        bucket: r.bucket,
      }),
    range,
    LIVE_MS,
    enabled,
  );
}

export function useMinerPayouts(address: string, limit?: number, before?: number, enabled = true) {
  return useBff<MinerPayoutsPage>(
    ["miner", address, "payouts", limit ?? null, before ?? null],
    bffUrl(`/api/v1/miners/${encodeURIComponent(address)}/payouts`, { limit, before }),
    LIVE_MS,
    enabled,
  );
}

export function useMinerRejects(address: string, enabled = true) {
  return useBff<RejectsResponse>(
    ["miner", address, "rejects"],
    bffUrl(`/api/v1/miners/${encodeURIComponent(address)}/rejects`),
    LIVE_MS,
    enabled,
  );
}

export function useMinerBalance(address: string, enabled = true) {
  return useBff<BalanceResponse>(
    ["miner", address, "balance"],
    bffUrl(`/api/v1/balance/${encodeURIComponent(address)}`),
    LIVE_MS,
    enabled,
  );
}

export function useFullRebate(address: string, enabled = true) {
  return useBff<FullRebateResponse>(
    ["miner", address, "full_rebate"],
    bffUrl(`/api/v1/full_rebate/${encodeURIComponent(address)}`),
    LIVE_MS,
    enabled,
  );
}
