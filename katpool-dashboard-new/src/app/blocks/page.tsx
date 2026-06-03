import type { Metadata } from "next";
import { PageHeader } from "@/components/dashboard/page-header";
import { BlocksTable } from "@/features/blocks/blocks-table";
import { BlocksSummary } from "@/features/overview/blocks-summary";

export const metadata: Metadata = { title: "Blocks" };

export default function BlocksPage() {
  return (
    <div className="space-y-6">
      <PageHeader title="Blocks" description="Every block the pool has found, with live lifecycle status." />
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        <div className="lg:col-span-2">
          <BlocksTable />
        </div>
        <BlocksSummary />
      </div>
    </div>
  );
}
