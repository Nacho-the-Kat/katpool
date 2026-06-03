import { NextResponse, type NextRequest } from "next/server";
import { serverEnv } from "@/lib/server/env";
import { fetchJson, UpstreamError, safeUrl } from "@/lib/server/upstream";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

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
