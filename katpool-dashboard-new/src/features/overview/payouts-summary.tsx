"use client";

import { Coins, Landmark, Sparkles } from "lucide-react";
import { Panel } from "@/components/dashboard/panel";
import { Separator } from "@/components/ui/separator";
import { ErrorState, LoadingRows } from "@/components/dashboard/states";
import { usePoolStats, useNetworkContext } from "@/lib/api/hooks";
import { formatKas, formatNumber, formatUsd, sompiToUsd } from "@/lib/format";

function Row({
  icon,
  label,
  value,
  sub,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  sub?: string;
}) {
  return (
    <div className="flex items-center justify-between gap-3 py-2.5">
      <span className="flex items-center gap-2.5 text-sm text-muted-foreground">
        <span className="flex size-8 items-center justify-center rounded-lg bg-muted text-foreground">{icon}</span>
        {label}
      </span>
      <span className="text-right">
        <span className="block text-sm font-semibold tnum">{value}</span>
        {sub ? <span className="block text-xs text-muted-foreground tnum">{sub}</span> : null}
      </span>
    </div>
  );
}

/** Confirmed payout totals + live treasury snapshot. */
export function PayoutsSummary() {
  const { data, isLoading, isError, refetch } = usePoolStats();
  const network = useNetworkContext();
  const kasUsd = network.data?.prices.kas_usd ?? null;

  return (
    <Panel title="Rewards & treasury" description="Confirmed distributions and current balances">
      {isError ? (
        <ErrorState onRetry={() => void refetch()} />
      ) : isLoading || !data ? (
        <LoadingRows rows={4} />
      ) : (
        <div className="divide-y divide-border/60">
          <Row
            icon={<Coins className="size-4" />}
            label="KAS paid (confirmed)"
            value={formatKas(data.payouts.kas_confirmed.kas)}
            sub={formatUsd(sompiToUsd(data.payouts.kas_confirmed.sompi, kasUsd))}
          />
          <Row
            icon={<Sparkles className="size-4" />}
            label="NACHO paid (KAS value)"
            value={formatKas(data.payouts.nacho_confirmed.kas)}
            sub={`${formatNumber(data.payouts.confirmed_payouts)} payouts`}
          />
          {data.treasury ? (
            <>
              <Separator className="my-1" />
              <Row
                icon={<Landmark className="size-4" />}
                label="Treasury KAS balance"
                value={formatKas(data.treasury.kas_balance.kas)}
                sub={formatUsd(sompiToUsd(data.treasury.kas_balance.sompi, kasUsd))}
              />
            </>
          ) : null}
        </div>
      )}
    </Panel>
  );
}
