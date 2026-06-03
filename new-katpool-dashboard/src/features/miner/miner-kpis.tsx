"use client";

import { Coins, Cpu, Gauge, ThumbsDown, ThumbsUp, Wallet } from "lucide-react";
import { StatCard } from "@/components/dashboard/stat-card";
import { useMinerProfile, useNetworkContext } from "@/lib/api/hooks";
import { formatHashrate, formatKas, formatNumber, formatUsd, sompiToUsd } from "@/lib/format";

/** KPI grid for a single miner. */
export function MinerKpis({ address }: { address: string }) {
  const { data, isLoading } = useMinerProfile(address);
  const network = useNetworkContext();
  const kasUsd = network.data?.prices.kas_usd ?? null;

  const total = (data?.accepted_shares ?? 0) + (data?.rejected_shares ?? 0);
  const rejectRate = total > 0 ? ((data?.rejected_shares ?? 0) / total) * 100 : 0;
  const payableUsd = data ? sompiToUsd(data.kas.payable.sompi, kasUsd) : null;

  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
      <StatCard
        label="Hashrate"
        icon={<Gauge className="size-4" />}
        value={data?.hashrate_hs ?? null}
        format={(v) => formatHashrate(v)}
        loading={isLoading}
        hint="Estimated from this miner's accepted shares in the window."
      />
      <StatCard
        label="Workers"
        icon={<Cpu className="size-4" />}
        value={data?.workers_count ?? null}
        format={(v) => formatNumber(Math.round(v))}
        loading={isLoading}
        hint="Distinct worker rigs ever seen for this wallet."
      />
      <StatCard
        label="Payable KAS"
        icon={<Wallet className="size-4" />}
        value={data ? Number(data.kas.payable.kas) : null}
        format={(v) => formatKas(String(v))}
        unit={payableUsd != null ? `≈ ${formatUsd(payableUsd)}` : undefined}
        loading={isLoading}
        colorIndex={1}
        hint="Allocated minus confirmed-paid: your current unpaid balance."
      />
      <StatCard
        label="Accepted shares"
        icon={<ThumbsUp className="size-4" />}
        value={data?.accepted_shares ?? null}
        format={(v) => formatNumber(Math.round(v))}
        loading={isLoading}
      />
      <StatCard
        label="Reject rate"
        icon={<ThumbsDown className="size-4" />}
        value={data ? rejectRate : null}
        format={(v) => `${v.toFixed(2)}%`}
        loading={isLoading}
        invertDelta
        hint="Share of submitted shares that were rejected in the window."
      />
      <StatCard
        label="NACHO pending"
        icon={<Coins className="size-4" />}
        value={data ? Number(data.nacho_rebate.pending.kas) : null}
        format={(v) => formatKas(String(v))}
        loading={isLoading}
        colorIndex={2}
        hint="Accrued NACHO rebate not yet paid, shown in its KAS value."
      />
    </div>
  );
}
