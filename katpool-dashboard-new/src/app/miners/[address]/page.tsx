import type { Metadata } from "next";
import { MinerDashboard } from "@/features/miner/miner-dashboard";
import { truncateMiddle } from "@/lib/format";

/** Decode the path segment, tolerating a malformed `%` sequence. */
function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ address: string }>;
}): Promise<Metadata> {
  const { address } = await params;
  return { title: `Miner ${truncateMiddle(safeDecode(address), 8, 6)}` };
}

export default async function MinerPage({ params }: { params: Promise<{ address: string }> }) {
  const { address } = await params;
  return <MinerDashboard address={safeDecode(address)} />;
}
