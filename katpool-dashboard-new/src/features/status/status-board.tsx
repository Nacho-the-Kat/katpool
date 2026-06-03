"use client";

import { CheckCircle2, CircleAlert, CircleSlash } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Panel } from "@/components/dashboard/panel";
import { usePoolStats, useNetworkContext } from "@/lib/api/hooks";
import { cn } from "@/lib/utils";

type Health = "ok" | "degraded" | "down";

function StatusPill({ state, label, detail }: { state: Health; label: string; detail: string }) {
  const Icon = state === "ok" ? CheckCircle2 : state === "degraded" ? CircleAlert : CircleSlash;
  const tone =
    state === "ok"
      ? "text-success bg-success/10 border-success/30"
      : state === "degraded"
        ? "text-warning bg-warning/10 border-warning/30"
        : "text-destructive bg-destructive/10 border-destructive/30";
  return (
    <Card className="flex items-center gap-4 p-5">
      <span className={cn("flex size-11 items-center justify-center rounded-xl border", tone)}>
        <Icon className="size-5" />
      </span>
      <div>
        <p className="font-medium">{label}</p>
        <p className="text-sm text-muted-foreground">{detail}</p>
      </div>
    </Card>
  );
}

/** Operational status board for the public API and network data sources. */
export function StatusBoard() {
  const stats = usePoolStats();
  const network = useNetworkContext();

  const poolState: Health = stats.isError ? "down" : stats.isLoading ? "degraded" : "ok";
  const degraded = network.data?.degraded ?? [];
  const netState: Health = network.isError
    ? "down"
    : degraded.length > 0
      ? "degraded"
      : network.isLoading
        ? "degraded"
        : "ok";

  return (
    <div className="space-y-6">
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <StatusPill
          state={poolState}
          label="Pool API"
          detail={
            poolState === "ok"
              ? "Serving live pool data"
              : poolState === "down"
                ? "Unreachable — retrying"
                : "Connecting…"
          }
        />
        <StatusPill
          state={netState}
          label="Network & price feeds"
          detail={
            netState === "down"
              ? "All upstream sources unavailable"
              : degraded.length > 0
                ? `Degraded: ${degraded.join(", ")}`
                : "Kaspa API + CoinGecko healthy"
          }
        />
      </div>

      <Panel title="About this data" description="How the dashboard sources its numbers">
        <ul className="space-y-2 text-sm text-muted-foreground">
          <li>
            <span className="text-foreground">Pool metrics</span> come from katpool&apos;s public,
            read-only v1 API (hashrate, blocks, payouts, miners, firmware).
          </li>
          <li>
            <span className="text-foreground">Network context</span> (hashrate, difficulty, supply,
            halving) is sourced from the Kaspa public API.
          </li>
          <li>
            <span className="text-foreground">Prices</span> (KAS, NACHO) come from CoinGecko. All
            on-chain amounts are computed with exact integer math — never floating point.
          </li>
          <li>Data refreshes automatically; on-chain figures lag the network by confirmation depth.</li>
        </ul>
      </Panel>
    </div>
  );
}
