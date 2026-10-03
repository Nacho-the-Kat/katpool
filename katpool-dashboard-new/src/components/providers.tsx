"use client";

import { useState, type ReactNode } from "react";
import { QueryClient, QueryClientProvider, keepPreviousData } from "@tanstack/react-query";
import { ThemeProvider } from "next-themes";
import { SearchFocusProvider } from "@/components/shell/search-focus";
import { DashboardApiError } from "@/lib/api/client";

/** App-wide client providers: theming + a single React Query client. */
export function Providers({ children }: { children: ReactNode }) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            // Fresh enough that a poll still moves, long enough that a route
            // change paints the last good frame instead of waiting on the API.
            staleTime: 8_000,
            gcTime: 5 * 60_000,
            // Tolerate transient *server* blips (Railway/BFF cold start, a
            // dropped poll, a 5xx) so a single miss never flashes a hard error
            // in place of a live panel — but never retry a 4xx. Retrying a 429
            // (rate limited) or other client error just amplifies load into a
            // retry storm that keeps the upstream's rate budget exhausted; we
            // let `keepPreviousData` hold the panel and wait for the next poll.
            // Same for gateway timeouts (503/504): re-firing a slow share-scan
            // history query only piles onto the DB that already timed out.
            retry: (failureCount, error) => {
              const status = error instanceof DashboardApiError ? error.status : 0;
              if (status >= 400 && status < 500) return false;
              if (status === 503 || status === 504) return false;
              return failureCount < 2;
            },
            retryDelay: (attempt) => Math.min(1_000 * 2 ** attempt, 15_000),
            // Refetch on focus. Hidden tabs do not poll. Coming back paints the
            // cached frame immediately (`keepPreviousData`) and refreshes only
            // when that frame is older than `staleTime`.
            refetchOnWindowFocus: true,
            refetchOnReconnect: true,
            // Keep the last good data on screen across refetches and range
            // changes instead of collapsing to skeletons/errors.
            placeholderData: keepPreviousData,
          },
        },
      }),
  );

  return (
    <ThemeProvider attribute="class" defaultTheme="dark" enableSystem disableTransitionOnChange>
      <QueryClientProvider client={client}>
        <SearchFocusProvider>{children}</SearchFocusProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
