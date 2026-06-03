import type { Metadata } from "next";
import { PageHeader } from "@/components/dashboard/page-header";
import { CyclesTable } from "@/features/payouts/cycles-table";
import { PayoutFlow } from "@/features/payouts/payout-flow";
import { PayoutsSummary } from "@/features/overview/payouts-summary";

export const metadata: Metadata = { title: "Payouts" };

export default function PayoutsPage() {
  return (
    <div className="space-y-6">
      <PageHeader title="Payouts" description="KAS and NACHO distribution cycles and treasury position." />
      <div className="grid grid-cols-1 items-stretch gap-6 lg:grid-cols-3">
        <div className="lg:col-span-2">
          <PayoutFlow />
        </div>
        <PayoutsSummary />
      </div>
      <CyclesTable />
    </div>
  );
}
