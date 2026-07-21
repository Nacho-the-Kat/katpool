import type { BucketToken } from "./api/types";

export type RangeKey = "1h" | "24h" | "7d" | "30d" | "90d" | "1y";

export interface ResolvedRange {
  from: string;
  to: string;
  bucket: BucketToken;
  windowSecs: number;
}

const SECONDS: Record<RangeKey, number> = {
  "1h": 3600,
  "24h": 86_400,
  "7d": 604_800,
  "30d": 2_592_000,
  "90d": 7_776_000,
  "1y": 31_536_000,
};

const BUCKET_SECONDS: Record<BucketToken, number> = {
  "1m": 60,
  "5m": 300,
  "1h": 3_600,
  "1d": 86_400,
};

/**
 * Align `from`/`to` to this quantum so concurrent pollers (and the API's
 * ~10s pool-cache TTL) share cache keys, while the window still slides.
 */
const RANGE_QUANTUM_MS = 10_000;

/**
 * API `MAX_WINDOW` for `?window=` endpoints (leaderboard, firmware, …).
 * Requests above this are silently capped server-side — never offer a
 * longer preset for windowed reads or the toggle will lie.
 */
export const MAX_WINDOW_SECS = 86_400;

/** Pick a bucket so a range yields a sensible, bounded number of points. */
export function bucketFor(key: RangeKey): BucketToken {
  switch (key) {
    case "1h":
      return "1m";
    case "24h":
      return "5m";
    case "7d":
      return "1h";
    default:
      return "1d";
  }
}

export function bucketSecs(token: BucketToken): number {
  return BUCKET_SECONDS[token];
}

/** All chart/history presets (API allows up to 1000 buckets; these stay under). */
export const RANGE_KEYS: RangeKey[] = ["1h", "24h", "7d", "30d", "90d", "1y"];

/**
 * Hashrate history presets that complete within the API/BFF latency budget.
 * 90d/1y remain valid API ranges but currently time out under production load
 * (share-table scan); re-enable once the covering index + timeout land and
 * measured p95 is under the API's 10s hard limit.
 */
export const HASHRATE_SERIES_RANGE_KEYS: RangeKey[] = ["1h", "24h", "7d", "30d"];

/** Presets safe for `?window=` endpoints (≤ {@link MAX_WINDOW_SECS}). */
export const WINDOW_RANGE_KEYS: RangeKey[] = ["1h", "24h"];

/**
 * Active-miners history does `COUNT DISTINCT` over `share` — heavier than
 * hashrate `SUM`. Cap at 30d so the toggle doesn't offer ranges that trip
 * the API hard timeout under load (90d/1y still typed but not offered here).
 */
export const MINERS_SERIES_RANGE_KEYS: RangeKey[] = ["1h", "24h", "7d", "30d"];

export const RANGE_LABELS: Record<RangeKey, string> = {
  "1h": "1H",
  "24h": "24H",
  "7d": "7D",
  "30d": "30D",
  "90d": "90D",
  "1y": "1Y",
};

/**
 * Resolve a range key to API params.
 *
 * `to` is quantized to {@link RANGE_QUANTUM_MS} (not bucket-floored) so:
 * - the API can still prorate the trailing open bucket
 * - pollers within the same quantum share the upstream cache key
 * - the window slides forward on each quantum tick (call this from the
 *   queryFn — do **not** freeze the result in `useMemo([range])`)
 */
export function resolveRange(key: RangeKey, nowMs = Date.now()): ResolvedRange {
  const span = SECONDS[key];
  const bucket = bucketFor(key);
  const toMs = Math.floor(nowMs / RANGE_QUANTUM_MS) * RANGE_QUANTUM_MS;
  const fromMs = toMs - span * 1000;
  return {
    from: new Date(fromMs).toISOString(),
    to: new Date(toMs).toISOString(),
    bucket,
    windowSecs: span,
  };
}

/** True when a range fits under the API's `?window=` cap. */
export function isWindowRange(key: RangeKey): boolean {
  return SECONDS[key] <= MAX_WINDOW_SECS;
}
