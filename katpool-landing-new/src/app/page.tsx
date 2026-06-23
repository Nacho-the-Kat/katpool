import { LandingApp } from "@/components/landing/landing-app";
import { fetchPoolStats } from "@/lib/pool-stats";
import { JsonLd } from "@/components/json-ld";
import { faqPageLd, serviceLd } from "@/lib/structured-data";

export default async function HomePage() {
  let initialStats = null;
  try {
    initialStats = await fetchPoolStats();
  } catch {
    // Client will retry; landing still renders without live numbers.
  }

  return (
    <>
      <JsonLd data={[serviceLd(), faqPageLd()]} />
      <LandingApp initialStats={initialStats} />
    </>
  );
}
