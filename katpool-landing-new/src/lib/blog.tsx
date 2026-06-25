import type { ReactNode } from "react";
import Link from "next/link";

/**
 * Lightweight, type-safe blog registry. Each post carries its metadata plus a
 * `Body` component for full JSX control (internal links, tables, etc.). The
 * index page, dynamic `[slug]` route, sitemap and OG images all read from here,
 * so adding a post is a single entry.
 */
export interface BlogPost {
  slug: string;
  title: string;
  description: string;
  datePublished: string;
  dateModified: string;
  readingMinutes: number;
  Body: () => ReactNode;
}

const link = "text-primary underline-offset-2 hover:underline";
const h2 = "text-xl font-semibold tracking-tight text-foreground";
const p = "text-muted-foreground";

export const BLOG_POSTS: BlogPost[] = [
  {
    slug: "how-to-choose-a-kaspa-mining-pool",
    title: "How to Choose a Kaspa Mining Pool in 2026",
    description:
      "A practical guide to picking a Kaspa (KAS) mining pool: how to weigh effective fees, reward schemes, open-source transparency, payout thresholds and server latency.",
    datePublished: "2026-06-25",
    dateModified: "2026-06-25",
    readingMinutes: 5,
    Body: () => (
      <>
        <p className={p}>
          Choosing a Kaspa mining pool comes down to five things: the effective fee after any rebates,
          the reward scheme, whether the pool is open source, the minimum payout, and server latency to
          your location. This guide walks through each so you can pick the pool that actually maximizes
          your take-home KAS — not just the one with the lowest advertised fee.
        </p>

        <h2 className={h2}>1. Compare effective fees, not headline fees</h2>
        <p className={p}>
          The advertised pool fee is rarely the whole story. What matters is the effective fee — what
          you actually keep after rebates. Most Kaspa pools charge 0.9–1% with no rebate, so their
          headline and effective fees are the same. Kat Pool charges a 0.75% topline fee but rebates
          33% of it as NACHO, for an effective fee around 0.5% — and 0% for holders of NACHO tokens,
          Nacho Kats NFTs or KATCLAIM NFTs. See the{" "}
          <Link href="/compare" className={link}>
            side-by-side pool comparison
          </Link>{" "}
          for the current numbers.
        </p>

        <h2 className={h2}>2. Understand the reward scheme</h2>
        <p className={p}>
          PROP (proportional) and PPLNS both reward consistent contribution, paying you a share of each
          block proportional to the work you submitted over a recent window. PPS/PPS+ pays a fixed rate
          per share for steadier income, usually at a higher fee. For most miners the difference is
          small as long as you stay on one pool; Kat Pool uses a transparent PROP scheme you can audit
          in the open-source code.
        </p>

        <h2 className={h2}>3. Prefer open-source, auditable pools</h2>
        <p className={p}>
          A pool decides how blocks are split and when you get paid. If that code is closed, you have to
          trust the operator. If it is open source, you can verify it. Kat Pool publishes its entire
          stack, so payout logic and fee handling are inspectable — a meaningful transparency edge over
          closed-source pools.
        </p>

        <h2 className={h2}>4. Check the minimum payout</h2>
        <p className={p}>
          A high minimum payout means small miners wait longer to get paid and risk dust building up.
          Kat Pool&apos;s minimum is 10 KAS, lower than the 50–100 KAS thresholds common elsewhere, so
          earnings reach your wallet sooner.
        </p>

        <h2 className={h2}>5. Mind latency and server coverage</h2>
        <p className={p}>
          The closer a stratum server is to your rig, the fewer stale shares you submit. Kat Pool runs a
          single anycast host that automatically routes each miner to the nearest of seven regions, so
          you get low latency without hunting for the right regional endpoint.
        </p>

        <h2 className={h2}>Putting it together</h2>
        <p className={p}>
          Run your hardware through the{" "}
          <Link href="/kaspa-mining-calculator" className={link}>
            Kaspa mining calculator
          </Link>{" "}
          to estimate earnings, read the{" "}
          <Link href="/kaspa-mining-pool" className={link}>
            full mining guide
          </Link>{" "}
          to set up, and if you&apos;re leaving a closed-source pool, the{" "}
          <Link href="/vs/humpool" className={link}>
            Kat Pool vs HumPool
          </Link>{" "}
          comparison shows exactly what changes when you switch.
        </p>
      </>
    ),
  },
];

export function getPost(slug: string): BlogPost | undefined {
  return BLOG_POSTS.find((post) => post.slug === slug);
}
