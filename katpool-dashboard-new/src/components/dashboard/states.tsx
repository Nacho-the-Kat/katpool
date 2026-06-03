"use client";

import { type ReactNode } from "react";
import { AlertTriangle, Inbox, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";

/** A centered empty-state with an icon and message. */
export function EmptyState({
  title,
  description,
  icon,
  className,
}: {
  title: string;
  description?: string;
  icon?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border px-6 py-12 text-center",
        className,
      )}
    >
      <div className="text-muted-foreground/60">{icon ?? <Inbox className="size-7" />}</div>
      <p className="text-sm font-medium">{title}</p>
      {description ? <p className="max-w-sm text-xs text-muted-foreground">{description}</p> : null}
    </div>
  );
}

/** An inline error panel with an optional retry. */
export function ErrorState({
  message,
  onRetry,
  className,
}: {
  message?: string;
  onRetry?: () => void;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-3 rounded-xl border border-destructive/30 bg-destructive/5 px-6 py-10 text-center",
        className,
      )}
    >
      <AlertTriangle className="size-6 text-destructive" />
      <p className="text-sm font-medium">Couldn&apos;t load this data</p>
      {message ? <p className="max-w-sm text-xs text-muted-foreground">{message}</p> : null}
      {onRetry ? (
        <Button variant="outline" size="sm" onClick={onRetry}>
          <RefreshCw className="size-3.5" /> Retry
        </Button>
      ) : null}
    </div>
  );
}

/** A block of shimmering rows for table/list loading. */
export function LoadingRows({ rows = 5, className }: { rows?: number; className?: string }) {
  return (
    <div className={cn("space-y-2", className)}>
      {Array.from({ length: rows }).map((_, i) => (
        <Skeleton key={i} className="h-11 w-full" />
      ))}
    </div>
  );
}
