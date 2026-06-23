import { NextResponse, type NextRequest } from "next/server";
import { serverEnv } from "@/lib/server/env";
import { fetchJson, UpstreamError, safeUrl } from "@/lib/server/upstream";

export const runtime = "nodejs";

// NB: deliberately NOT `dynamic = "force-dynamic"`. In Next 15 that is
// equivalent to `fetchCache = "force-no-store"`, which would override the
// per-fetch `next: { revalidate }` below and send every browser poll straight
// through to the upstream API. The handler is already dynamic (it reads
// `req.nextUrl`), so leaving the default `fetchCache: "auto"` lets the upstream
// fetch hit the Next Data Cache — collapsing N concurrent viewers (and each
// tab's 15s polls) into ~1 upstream request per endpoint per revalidate window.
// That coalescing is what keeps the upstream's per-IP rate budget intact.

/**
 * Same-origin read-only proxy to the katpool v1 API.
 *
 * The browser never talks to the public API directly: this keeps the data
 * surface same-origin (no CORS, no client-side base-URL leakage) and lets us
 * apply short edge revalidation in front of the API's own TTL cache. Only the
 * read-only `/pool`, `/miners`, `/balance`, `/full_rebate` trees are
 * forwarded; anything else 404s.
 */
const ALLOWED_PREFIXES = ["pool", "miners", "balance", "full_rebate"] as const;

/** Per-tree revalidation (seconds), tuned to the upstream cache TTLs. */
function revalidateFor(first: string): number {
  return first === "balance" || first === "miners" ? 5 : 10;
}

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
    const data = await fetchJson<unknown>(target, { revalidate: revalidateFor(first) });
    return NextResponse.json(data, {
      headers: {
        "Cache-Control": `public, s-maxage=${revalidateFor(first)}, stale-while-revalidate=30`,
      },
    });
  } catch (err) {
    const status = err instanceof UpstreamError ? err.status : 502;

    // Pass 429 through verbatim (with Retry-After) rather than masking it as a
    // 502. A 502 reads as a hard fault and triggers the client's retry path,
    // which would pile more requests onto an already-throttled upstream; a 429
    // tells React Query to stop and back off until the next scheduled poll.
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
    return NextResponse.json(
      {
        error: {
          code: status === 404 ? "not_found" : "upstream_error",
          message: status === 404 ? "not found" : "upstream unavailable",
        },
      },
      { status: status === 404 ? 404 : 502 },
    );
  }
}
