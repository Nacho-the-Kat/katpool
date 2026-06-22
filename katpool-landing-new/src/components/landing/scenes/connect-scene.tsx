"use client";

import { motion } from "framer-motion";
import { Cable, Server } from "lucide-react";
import { miningConfig } from "@/lib/mining";
import { CopyButton } from "../copy-button";

export function ConnectScene() {
  const { host, recommended, ports } = miningConfig();
  const stratumUrl = `stratum+tcp://${host}:${recommended.port}`;

  return (
    <div className="mx-auto w-full max-w-5xl">
      <div className="text-center lg:text-left">
        <motion.p
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          className="text-xs uppercase tracking-[0.2em] text-primary"
        >
          Connect in seconds
        </motion.p>
        <motion.h2
          initial={{ opacity: 0, y: 16 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.06 }}
          className="mt-3 text-3xl font-semibold tracking-tight sm:text-4xl"
        >
          Point your miner.{" "}
          <span className="text-grad">Get blocks.</span>
        </motion.h2>
      </div>

      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.12 }}
        className="glass-panel mt-8 rounded-3xl p-5 sm:p-7"
      >
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <p className="text-xs uppercase tracking-wider text-muted-foreground">Recommended endpoint</p>
            <p className="mt-1 font-mono text-lg sm:text-xl">{stratumUrl}</p>
            <p className="mt-1 text-xs text-muted-foreground">
              Wallet as username · worker name optional · vardiff from seed {recommended.seed}
            </p>
          </div>
          <CopyButton value={stratumUrl} label="Copy stratum URL" />
        </div>

        <div className="mt-6 grid gap-3 border-t border-border/60 pt-6 sm:grid-cols-2">
          <div className="flex items-start gap-3">
            <Server className="mt-0.5 size-4 text-primary" />
            <div>
              <p className="text-sm font-medium">Host</p>
              <p className="font-mono text-sm text-muted-foreground">{host}</p>
            </div>
          </div>
          <div className="flex items-start gap-3">
            <Cable className="mt-0.5 size-4 text-primary" />
            <div>
              <p className="text-sm font-medium">Default port</p>
              <p className="font-mono text-sm text-muted-foreground">{recommended.port}</p>
            </div>
          </div>
        </div>
      </motion.div>

      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.2 }}
        className="mt-5"
      >
        <p className="mb-3 text-center text-xs uppercase tracking-wider text-muted-foreground lg:text-left">
          All stratum ports (vardiff enabled)
        </p>
        <div className="grid grid-cols-4 gap-2 sm:grid-cols-8">
          {ports.map((p, i) => (
            <motion.div
              key={p.port}
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              transition={{ delay: 0.22 + i * 0.04 }}
              className={`rounded-xl border px-2 py-2 text-center ${
                p.port === recommended.port
                  ? "border-primary/50 bg-primary/10"
                  : "border-border/60 bg-card/30"
              }`}
            >
              <p className="metric text-sm font-semibold">{p.port}</p>
              <p className="text-[10px] text-muted-foreground">seed {p.seed}</p>
            </motion.div>
          ))}
        </div>
      </motion.div>
    </div>
  );
}
