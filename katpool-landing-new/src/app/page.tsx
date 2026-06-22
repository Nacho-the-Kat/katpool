import { LandingApp } from "@/components/landing/landing-app";
import { fetchPoolStats } from "@/lib/pool-stats";

export default async function HomePage() {
  let initialStats = null;
  try {
    initialStats = await fetchPoolStats();
  } catch {
    // Client will retry; landing still renders without live numbers.
  }

  return <LandingApp initialStats={initialStats} />;
}
