/**
 * Schema.org JSON-LD builders for rich results and AI answer-engine ingestion.
 *
 * Every claim here mirrors the live product facts surfaced elsewhere in the app
 * (fee model in `fees-scene`, topology in `mining.ts`) so the structured data
 * never drifts from what miners actually see.
 */
import { APP_URL, GITHUB_URL, TWITTER_URL } from "./mining";

const SITE_URL = process.env.NEXT_PUBLIC_SITE_URL ?? "https://katpool.com";
const ORG_ID = `${SITE_URL}/#organization`;
const SITE_ID = `${SITE_URL}/#website`;

export function organizationLd() {
  return {
    "@context": "https://schema.org",
    "@type": "Organization",
    "@id": ORG_ID,
    name: "Kat Pool",
    alternateName: "katpool",
    url: SITE_URL,
    logo: `${SITE_URL}/icon-512.png`,
    description:
      "Open-source Kaspa (KAS) mining pool with global anycast stratum, transparent PROP (proportional) payouts and NACHO fee rebates.",
    sameAs: [TWITTER_URL, GITHUB_URL],
  };
}

export function websiteLd() {
  return {
    "@context": "https://schema.org",
    "@type": "WebSite",
    "@id": SITE_ID,
    name: "Kat Pool",
    url: SITE_URL,
    publisher: { "@id": ORG_ID },
    inLanguage: "en",
  };
}

/** The pool itself, modelled as an offered Service for rich understanding. */
export function serviceLd() {
  return {
    "@context": "https://schema.org",
    "@type": "Service",
    name: "Kat Pool — Kaspa Mining Pool",
    serviceType: "Cryptocurrency mining pool",
    provider: { "@id": ORG_ID },
    url: SITE_URL,
    areaServed: "Worldwide",
    description:
      "Mine Kaspa (KAS) on an open-source pool: global anycast stratum across 7 regions, PROP (proportional) payouts from a 10 KAS minimum, and NACHO rebates that drop the effective fee to as low as 0%.",
    offers: {
      "@type": "Offer",
      priceCurrency: "KAS",
      description:
        "0.75% topline pool fee with 33% rebated in NACHO (≈0.5% effective); NACHO token and Nacho Kats / KATCLAIM NFT holders receive 100% of the fee back, for a 0% effective fee.",
    },
    category: "Cryptocurrency mining",
  };
}

interface FaqItem {
  question: string;
  answer: string;
}

const FAQ_ITEMS: FaqItem[] = [
  {
    question: "What is Kat Pool?",
    answer:
      "Kat Pool is a fully open-source Kaspa (KAS) mining pool. It runs a globally distributed, anycast stratum across seven regions, pays miners on a transparent PROP (proportional) scheme — each matured block's reward is shared by contribution over a recent-share window — and rebates part of the pool fee as NACHO.",
  },
  {
    question: "How much does Kat Pool cost? What is the fee?",
    answer:
      "The standard pool fee is 0.75%, of which 33% is rebated to every miner in NACHO — an effective fee of about 0.5%. Holders of NACHO tokens (100M+), Nacho Kats NFTs, or KATCLAIM NFTs receive 100% of the fee back as NACHO, making their effective fee 0%.",
  },
  {
    question: "Is Kat Pool a good HumPool alternative?",
    answer:
      "Yes. Kat Pool is open source, charges a lower effective fee than HumPool's 1%, supports all Kaspa ASIC models (IceRiver KS series, Bitmain KS/KA series), and uses a comparable proportional reward model over a recent-share window — so migrating from HumPool's PPLNS is straightforward.",
  },
  {
    question: "Which payout scheme and minimum payout does Kat Pool use?",
    answer:
      "Kat Pool uses a PROP (proportional) reward scheme: each matured block's reward is shared across miners by their contribution over a recent-share window. The minimum payout is 10 KAS, and a portion of fees is returned as NACHO (KRC-20) at each payout cycle.",
  },
  {
    question: "How do I start mining Kaspa on Kat Pool?",
    answer:
      "Point your miner at the anycast stratum host kas.katpool.com (ports 1111–8888, vardiff on every port), set your username to your Kaspa wallet address, and start hashing. Anycast routes you to the nearest of seven global regions automatically.",
  },
  {
    question: "Is Kat Pool open source?",
    answer:
      "Yes — the entire Kat Pool stack is open source on GitHub. You can audit the code, verify payouts, or run your own node.",
  },
];

export function faqPageLd() {
  return {
    "@context": "https://schema.org",
    "@type": "FAQPage",
    mainEntity: FAQ_ITEMS.map((item) => ({
      "@type": "Question",
      name: item.question,
      acceptedAnswer: { "@type": "Answer", text: item.answer },
    })),
  };
}

export const appUrl = APP_URL;
