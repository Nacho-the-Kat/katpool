import { NextResponse, type NextRequest } from "next/server";
import { serverEnv } from "@/lib/server/env";
import { fetchJson, UpstreamError, safeUrl } from "@/lib/server/upstream";

export const runtime = "nodejs";

/**
 * Same-origin read-only proxy to the katpool v1 API.
 *
 * The browser never talks to the public API directly: this keeps the data
 * surface same-origin (no CORS, no client-side base-URL leakage). Responses
 * are marked no-store so Cloudflare and other shared caches never serve stale
 * JSON to live polls.
 */
const ALLOWED_PREFIXES = ["pool", "miners", "balance", "full_rebate"] as const;

/** Per-tree upstream coalesce window (seconds), tuned to the API TTL caches. */
function revalidateFor(first: string): number {
  return first === "balance" || first === "miners" ? 5 : 10;
}

/**
 * History series can approach the API's 10s hard timeout (pool-wide share
 * scans). Stay above that so we surface the API's 503 rather than aborting
 * first and returning a Cloudflare-masked 502. Non-series reads stay snappy.
 */
function timeoutFor(path: string[]): number {
  return path.includes("history") ? 15_000 : 8_000;
}

function isAbortError(err: unknown): boolean {
  return (
    (err instanceof Error && err.name === "AbortError") ||
    (typeof DOMException !== "undefined" &&
      err instanceof DOMException &&
      err.name === "AbortError")
  );
}

const LIVE_HEADERS = {
  "Cache-Control": "private, no-store, no-cache, must-revalidate",
  "CDN-Cache-Control": "no-store",
} as const;

export async function GET(
  req: NextRequest,
  ctx: { params: Promise<{ path: string[] }> },
): Promise<NextResponse> {
  const { path } = await ctx.params;
  const first = path[0] ?? "";
  if (!ALLOWED_PREFIXES.includes(first as (typeof ALLOWED_PREFIXES)[number])) {
    return NextResponse.json(
      { error: { code: "not_found", message: "not found" } },
      { status: 404 },
    );
  }

  const search = req.nextUrl.search;
  const target = `${serverEnv.katpoolApiBaseUrl()}/${path.map(encodeURIComponent).join("/")}${search}`;

  try {
    const data = await fetchJson<unknown>(target, {
      revalidate: revalidateFor(first),
      timeoutMs: timeoutFor(path),
    });
    return NextResponse.json(data, { headers: LIVE_HEADERS });
  } catch (err) {
    // Abort ⇒ we gave up waiting; map to 504 so it isn't confused with a dead
    // upstream. Pass 429 through verbatim (with Retry-After) rather than
    // masking it as a 502 — a 502 reads as a hard fault and triggers the
    // client's retry path, which would pile more requests onto an already-
    // throttled upstream; a 429 tells React Query to stop and back off until
    // the next scheduled poll.
    const status = isAbortError(err)
      ? 504
      : err instanceof UpstreamError
        ? err.status
        : 502;

    if (status === 429) {
      const retryAfter = err instanceof UpstreamError ? err.retryAfter : undefined;
      return NextResponse.json(
        { error: { code: "rate_limited", message: "rate limited" } },
        { status: 429, headers: retryAfter ? { "Retry-After": retryAfter } : undefined },
      );
    }

    if (status >= 500) {
      console.error("v1 proxy error", { target: safeUrl(target), status });
    }

    const notFound = status === 404;
    // API statement/request timeout is 503; BFF abort is 504. Surface both as
    // 504 so the client can skip retry storms on expensive series scans.
    const timedOut = status === 503 || status === 504;
    return NextResponse.json(
      {
        error: {
          code: notFound ? "not_found" : timedOut ? "timeout" : "upstream_error",
          message: notFound
            ? "not found"
            : timedOut
              ? "upstream timed out"
              : "upstream unavailable",
        },
      },
      { status: notFound ? 404 : timedOut ? 504 : 502 },
    );
  }
}
