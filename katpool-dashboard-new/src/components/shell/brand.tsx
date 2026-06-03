import Image from "next/image";
import Link from "next/link";
import { cn } from "@/lib/utils";

const POOL_NAME = process.env.NEXT_PUBLIC_POOL_NAME ?? "katpool";

/** Wordmark + glyph used in the sidebar and mobile header. */
export function Brand({ className }: { className?: string }) {
  return (
    <Link href="/" className={cn("flex items-center gap-2.5", className)} aria-label={`${POOL_NAME} home`}>
      <Image
        src="/brand/katpool-icon.png"
        alt=""
        width={32}
        height={32}
        priority
        className="size-8 rounded-lg shadow-[var(--shadow-glow)]"
      />
      <span className="text-base font-semibold tracking-tight">
        {POOL_NAME}
        <span className="text-primary">.</span>
      </span>
    </Link>
  );
}
