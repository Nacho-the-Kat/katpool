import Link from "next/link";
import { cn } from "@/lib/utils";

const POOL_NAME = process.env.NEXT_PUBLIC_POOL_NAME ?? "katpool";

/** Wordmark + glyph used in the sidebar and mobile header. */
export function Brand({ className }: { className?: string }) {
  return (
    <Link href="/" className={cn("flex items-center gap-2.5", className)} aria-label={`${POOL_NAME} home`}>
      <span className="relative flex size-8 items-center justify-center rounded-lg bg-gradient-to-br from-primary to-secondary text-sm font-bold text-primary-foreground shadow-[var(--shadow-glow)]">
        ⛏
      </span>
      <span className="text-base font-semibold tracking-tight">
        {POOL_NAME}
        <span className="text-primary">.</span>
      </span>
    </Link>
  );
}
