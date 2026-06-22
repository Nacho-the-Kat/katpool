"use client";

import { motion } from "framer-motion";
import { miningConfig } from "@/lib/mining";
import { EDGE_REGIONS } from "@/lib/edge-regions";
import { EdgeGlobe } from "../edge-globe";

export function EdgeScene() {
  const { host } = miningConfig();

  return (
    <div className="mx-auto grid w-full max-w-6xl items-center gap-6 lg:grid-cols-[0.95fr_1.05fr] lg:gap-10">
      <div className="order-2 lg:order-1">
        <motion.p
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          className="text-xs uppercase tracking-[0.2em] text-primary"
        >
          Global infrastructure
        </motion.p>
        <motion.h2
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.06 }}
          className="mt-3 text-3xl font-semibold tracking-tight sm:text-4xl lg:text-[2.75rem] lg:leading-[1.08]"
        >
          Seven edge regions.
          <br />
          <span className="text-grad">One stratum URL.</span>
        </motion.h2>
        <motion.p
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.12 }}
          className="mt-4 max-w-md text-sm leading-relaxed text-muted-foreground sm:text-base"
        >
          Fly.io anycast routes hashrate to the nearest healthy edge. Use{" "}
          <span className="font-mono text-foreground">{host}</span> for automatic routing, or
          pin a regional host below.
        </motion.p>

        <motion.div
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.18 }}
          className="mt-6 space-y-1.5"
        >
          {EDGE_REGIONS.map((r, i) => (
            <motion.div
              key={r.host}
              initial={{ opacity: 0, x: -12 }}
              animate={{ opacity: 1, x: 0 }}
              transition={{ delay: 0.22 + i * 0.04 }}
              className="group flex items-center gap-3 rounded-xl border border-transparent px-3 py-2.5 transition hover:border-border/50 hover:bg-card/35"
            >
              <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-primary/20 bg-primary/8 font-mono text-[10px] font-medium uppercase tracking-wide text-primary">
                {r.fly}
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-foreground">{r.label}</p>
                <p className="truncate font-mono text-[11px] text-muted-foreground">{r.host}</p>
              </div>
              <span className="live-dot size-1.5 shrink-0 rounded-full bg-success opacity-0 transition group-hover:opacity-100" />
            </motion.div>
          ))}
        </motion.div>
      </div>

      <motion.div
        initial={{ opacity: 0, scale: 0.94 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ delay: 0.08, duration: 0.7, ease: [0.22, 1, 0.36, 1] }}
        className="order-1 flex flex-col items-center lg:order-2"
      >
        <EdgeGlobe />

        <div className="mt-2 flex flex-wrap items-center justify-center gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
          <span className="inline-flex items-center gap-1.5">
            <span className="size-2 rounded-full bg-[#49eacb]" />
            Origin · Germany
          </span>
          <span className="inline-flex items-center gap-1.5">
            <span className="size-2 rounded-full bg-[#70c7ba]" />
            Edge · 7 regions
          </span>
          <span className="font-mono text-foreground/80">{host}</span>
        </div>
      </motion.div>
    </div>
  );
}
