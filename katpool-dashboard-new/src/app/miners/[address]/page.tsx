import type { Metadata } from "next";
import { MinerDashboard } from "@/features/miner/miner-dashboard";
import { truncateMiddle } from "@/lib/format";

export async function generateMetadata({
  params,
}: {
  params: Promise<{ address: string }>;
}): Promise<Metadata> {
  const { address } = await params;
  return { title: `Miner ${truncateMiddle(decodeURIComponent(address), 8, 6)}` };
}

export default async function MinerPage({ params }: { params: Promise<{ address: string }> }) {
  const { address } = await params;
  return <MinerDashboard address={decodeURIComponent(address)} />;
}
