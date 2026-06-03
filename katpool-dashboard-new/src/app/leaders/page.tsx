import type { Metadata } from "next";
import { PageHeader } from "@/components/dashboard/page-header";
import { LeaderboardTable } from "@/features/leaders/leaderboard-table";

export const metadata: Metadata = { title: "Leaderboard" };

export default function LeadersPage() {
  return (
    <div className="space-y-6">
      <PageHeader
        title="Leaderboard"
        description="The pool's top contributing miners, ranked by hashrate over your selected window."
      />
      <LeaderboardTable limit={100} />
    </div>
  );
}
