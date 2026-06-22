"use client";

import { motion } from "framer-motion";
import { ArrowDown, Blocks, Gauge, Percent, Timer } from "lucide-react";
import type { MiningPoolStats } from "@/lib/pool-stats";
import { formatBlockCount, formatRelativeTime, parseHashRate } from "@/lib/pool-stats";
import { APP_URL } from "@/lib/mining";

interface HeroSceneProps {
  stats: MiningPoolStats | null;
  onNext: () => void;
}

function StatCard({
  icon: Icon,
  label,
  value,
  sub,
  delay,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: string;
  sub?: string;
  delay: number;
}) {
  return (
    <motion.div
      initial={{ opacity: 0, y: 16 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay, duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
      className="glass-panel rounded-2xl p-4 sm:p-5"
    >
      <div className="mb-3 flex items-center gap-2 text-xs uppercase tracking-wider text-muted-foreground">
        <Icon className="size-3.5 text-primary" />
        {label}
      </div>
      <p className="metric text-2xl font-semibold sm:text-3xl">{value}</p>
      {sub && <p className="mt-1 text-xs text-muted-foreground">{sub}</p>}
    </motion.div>
  );
}

export function HeroScene({ stats, onNext }: HeroSceneProps) {
  const hr = stats ? parseHashRate(stats.current_hashRate) : null;

  return (
    <div className="mx-auto grid w-full max-w-6xl gap-8 lg:grid-cols-[1.1fr_0.9fr] lg:items-center">
      <div>
        <motion.div
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="mb-5 inline-flex items-center gap-2 rounded-full border border-border bg-card/50 px-3 py-1.5 text-xs text-muted-foreground backdrop-blur-sm"
        >
          <span className="live-dot size-2 rounded-full bg-success" />
          Live mainnet pool · PROP payouts
        </motion.div>

        <motion.h1
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.08, duration: 0.55 }}
          className="max-w-xl text-4xl font-semibold leading-[1.05] tracking-tight sm:text-5xl lg:text-6xl"
        >
          Mine Kaspa at the{" "}
          <span className="text-grad">edge</span>
        </motion.h1>

        <motion.p
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.16, duration: 0.5 }}
          className="mt-5 max-w-lg text-base leading-relaxed text-muted-foreground sm:text-lg"
        >
          Open-source stratum with global anycast, transparent PROP rewards, and NACHO fee rebates.
          Built for serious Kaspa hashrate — not another bloated dashboard.
        </motion.p>

        <motion.div
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.24, duration: 0.5 }}
          className="mt-8 flex flex-wrap items-center gap-3"
        >
          <a
            href={APP_URL}
            className="inline-flex items-center gap-2 rounded-full bg-primary px-6 py-3 text-sm font-medium text-primary-foreground shadow-[0_0_48px_oklch(0.82_0.15_184/30%)] transition hover:brightness-110"
          >
            Start mining
            <ArrowDown className="size-4 rotate-[-90deg]" />
          </a>
          <button
            type="button"
            onClick={onNext}
            className="inline-flex items-center gap-2 rounded-full border border-border px-5 py-3 text-sm text-muted-foreground transition hover:border-primary/40 hover:text-foreground"
          >
            Explore the pool
            <ArrowDown className="size-4" />
          </button>
        </motion.div>
      </div>

      <div className="grid grid-cols-2 gap-3 sm:gap-4">
        <StatCard
          icon={Gauge}
          label="Pool hashrate"
          value={hr ? `${hr.value}` : "—"}
          sub={hr?.unit ?? "syncing…"}
          delay={0.2}
        />
        <StatCard
          icon={Blocks}
          label="Blocks found"
          value={stats ? formatBlockCount(stats.totalBlocksCount) : "—"}
          sub="all-time mainnet"
          delay={0.28}
        />
        <StatCard
          icon={Percent}
          label="Listed fee"
          value={stats ? `${stats.poolFee}%` : "0.5%"}
          sub="PROP · min 10 KAS payout"
          delay={0.36}
        />
        <StatCard
          icon={Timer}
          label="Last block"
          value={stats ? formatRelativeTime(stats.lastblocktime) : "—"}
          sub={stats?.feeType ?? "PROP"}
          delay={0.44}
        />
      </div>
    </div>
  );
}
