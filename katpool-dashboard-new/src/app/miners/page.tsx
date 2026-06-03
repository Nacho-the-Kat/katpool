import type { Metadata } from "next";
import { Cpu } from "lucide-react";
import { PageHeader } from "@/components/dashboard/page-header";
import { Card } from "@/components/ui/card";
import { WalletSearch } from "@/components/shell/wallet-search";
import { LeaderboardTable } from "@/features/leaders/leaderboard-table";

export const metadata: Metadata = { title: "Miner Lookup" };

export default function MinersPage() {
  return (
    <div className="space-y-6">
      <PageHeader title="Miner Lookup" description="Search any wallet to see its hashrate, workers, balance and payout history." />
      <Card className="flex flex-col items-center gap-4 p-8 text-center">
        <span className="flex size-12 items-center justify-center rounded-xl bg-primary/10 text-primary">
          <Cpu className="size-6" />
        </span>
        <div>
          <h2 className="text-lg font-semibold">Enter a wallet address</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            Paste a <span className="font-mono">kaspa:</span> address to open its live dashboard.
          </p>
        </div>
        <WalletSearch className="max-w-lg" />
      </Card>
      <LeaderboardTable limit={10} compact />
    </div>
  );
}
