"use client";

import { motion } from "framer-motion";
import { Globe2, Zap } from "lucide-react";
import { miningConfig } from "@/lib/mining";

const REGION_COORDS: Record<string, { x: number; y: number }> = {
  "na-west": { x: 14, y: 38 },
  "na-east": { x: 26, y: 36 },
  eu: { x: 48, y: 30 },
  ap: { x: 72, y: 52 },
  hkg: { x: 76, y: 44 },
  sa: { x: 30, y: 68 },
  au: { x: 82, y: 72 },
};

export function EdgeScene() {
  const { regions, host } = miningConfig();
  const edgeRegions = regions.filter((r) => !r.primary);

  return (
    <div className="mx-auto grid w-full max-w-6xl gap-8 lg:grid-cols-[1fr_1.1fr] lg:items-center">
      <div>
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
          className="mt-3 text-3xl font-semibold tracking-tight sm:text-4xl lg:text-5xl"
        >
          Seven regions.
          <br />
          <span className="text-grad">One stratum URL.</span>
        </motion.h2>
        <motion.p
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.12 }}
          className="mt-5 max-w-md text-muted-foreground"
        >
          Fly.io anycast routes miners to the nearest edge automatically. Point your rig at{" "}
          <span className="font-mono text-foreground">{host}</span> or pick a regional host for
          deterministic routing.
        </motion.p>

        <motion.ul
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.2 }}
          className="mt-6 grid gap-2 sm:grid-cols-2"
        >
          {edgeRegions.map((r, i) => (
            <li
              key={r.host}
              className="flex items-center gap-2 rounded-xl border border-border/60 bg-card/40 px-3 py-2 text-sm"
            >
              <span className="size-1.5 rounded-full bg-primary" style={{ animationDelay: `${i * 0.15}s` }} />
              <span className="text-muted-foreground">{r.label}</span>
              <span className="ml-auto truncate font-mono text-xs text-foreground/80">{r.host}</span>
            </li>
          ))}
        </motion.ul>
      </div>

      <motion.div
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ delay: 0.1, duration: 0.6 }}
        className="glass-panel relative aspect-[4/3] overflow-hidden rounded-3xl p-6"
      >
        <div className="absolute inset-0 bg-[radial-gradient(circle_at_50%_40%,oklch(0.82_0.15_184/12%),transparent_65%)]" />

        {/* Stylized world map dots */}
        <svg viewBox="0 0 100 80" className="relative h-full w-full" aria-hidden>
          <defs>
            <radialGradient id="pulse" cx="50%" cy="50%" r="50%">
              <stop offset="0%" stopColor="oklch(0.82 0.15 184)" stopOpacity="0.9" />
              <stop offset="100%" stopColor="oklch(0.82 0.15 184)" stopOpacity="0" />
            </radialGradient>
          </defs>
          {/* Abstract continent silhouettes */}
          <ellipse cx="22" cy="38" rx="14" ry="18" fill="oklch(1 0 0 / 4%)" />
          <ellipse cx="50" cy="32" rx="12" ry="16" fill="oklch(1 0 0 / 4%)" />
          <ellipse cx="74" cy="48" rx="16" ry="14" fill="oklch(1 0 0 / 4%)" />
          <ellipse cx="82" cy="70" rx="8" ry="6" fill="oklch(1 0 0 / 4%)" />

          {edgeRegions.map((r, i) => {
            const prefix = r.host.split(".")[0] ?? "";
            const pos = REGION_COORDS[prefix] ?? { x: 50, y: 40 };
            return (
              <g key={r.host}>
                <motion.circle
                  cx={pos.x}
                  cy={pos.y}
                  r="6"
                  fill="url(#pulse)"
                  initial={{ opacity: 0.3, scale: 0.6 }}
                  animate={{ opacity: [0.3, 0.7, 0.3], scale: [0.6, 1.1, 0.6] }}
                  transition={{ duration: 2.8, repeat: Infinity, delay: i * 0.25 }}
                />
                <circle cx={pos.x} cy={pos.y} r="1.8" fill="oklch(0.82 0.15 184)" />
              </g>
            );
          })}

          {/* Center anycast hub */}
          <motion.circle
            cx="50"
            cy="42"
            r="3.5"
            fill="oklch(0.83 0.14 78)"
            animate={{ scale: [1, 1.15, 1] }}
            transition={{ duration: 2, repeat: Infinity }}
          />
        </svg>

        <div className="absolute bottom-4 left-4 right-4 flex items-center justify-between rounded-xl border border-border/50 bg-background/50 px-3 py-2 text-xs backdrop-blur-sm">
          <span className="inline-flex items-center gap-1.5 text-muted-foreground">
            <Globe2 className="size-3.5 text-primary" />
            Anycast origin
          </span>
          <span className="inline-flex items-center gap-1.5 font-mono text-foreground">
            <Zap className="size-3.5 text-secondary" />
            {host}
          </span>
        </div>
      </motion.div>
    </div>
  );
}
