"use client";

import { useState, type ReactNode } from "react";
import { QueryClient, QueryClientProvider, keepPreviousData } from "@tanstack/react-query";
import { ThemeProvider } from "next-themes";
import { SearchFocusProvider } from "@/components/shell/search-focus";

/** App-wide client providers: theming + a single React Query client. */
export function Providers({ children }: { children: ReactNode }) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: {
            staleTime: 10_000,
            gcTime: 5 * 60_000,
            // Tolerate transient upstream blips (Railway/BFF cold start, a
            // dropped poll) so a single miss never flashes a hard error in
            // place of a live panel. Capped exponential backoff.
            retry: 3,
            retryDelay: (attempt) => Math.min(1_000 * 2 ** attempt, 15_000),
            refetchOnWindowFocus: false,
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
