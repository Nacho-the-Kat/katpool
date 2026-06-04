import Link from "next/link";
import {
  ArrowRight,
  Cpu,
  Gauge,
  Globe2,
  KeyRound,
  LineChart,
  Plug,
  ShieldCheck,
  Wallet,
} from "lucide-react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Panel } from "@/components/dashboard/panel";
import { CopyButton } from "@/components/dashboard/copy-button";
import { miningConfig } from "@/lib/mining";
import { formatNumber } from "@/lib/format";

/** Popular kHeavyHash ASICs surfaced on the connect card. */
const ASIC_MODELS = [
  "IceRiver KS-series",
  "Bitmain Antminer KS3 / KS5",
  "Goldshell KA-series",
] as const;

/** A labelled, copyable monospace field (connection settings). */
function CopyField({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-border bg-background/60 px-3 py-2">
      <div className="min-w-0">
        <p className="text-[0.6875rem] uppercase tracking-[0.1em] text-muted-foreground">{label}</p>
        <p className="truncate font-mono text-sm text-foreground" title={value}>
          {value}
        </p>
      </div>
      <CopyButton value={value} label={`Copy ${label}`} />
    </div>
  );
}

function Step({
  n,
  icon: Icon,
  title,
  children,
}: {
  n: number;
  icon: typeof Wallet;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <Card className="flex h-full flex-col gap-3 p-5">
      <div className="flex items-center gap-3">
        <span className="flex size-9 items-center justify-center rounded-xl border border-primary/30 bg-primary/10 text-primary">
          <Icon className="size-5" />
        </span>
        <span className="text-[0.6875rem] font-medium uppercase tracking-[0.14em] text-muted-foreground">
          Step {n}
        </span>
      </div>
      <h3 className="text-base font-semibold tracking-tight">{title}</h3>
      <div className="text-sm leading-relaxed text-muted-foreground [&_a]:text-primary [&_a:hover]:underline">
        {children}
      </div>
    </Card>
  );
}

/**
 * "Start mining" — the flagship onboarding guide. Connection facts come from
 * {@link miningConfig} (env-overridable; defaults from the verified cutover
 * topology), so the page is always accurate for the deployment it ships in.
 */
export function StartGuide() {
  const cfg = miningConfig();
  const primary = cfg.primary;
  const recommendedPort = cfg.recommended;
  const addressExample = `${cfg.addressPrefix}:your-wallet-address`;
  const userExample = `${addressExample}.rig1`;
  const stratumUrl = `stratum+tcp://${primary.host}:${recommendedPort.port}`;
  const isTestnet = cfg.network === "testnet-10";

  return (
    <div className="space-y-6">
      {/* CTA hero */}
      <Card className="relative overflow-hidden">
        <div className="pointer-events-none absolute inset-0 app-aurora opacity-80" />
        <div className="pointer-events-none absolute -right-24 -top-24 size-72 rounded-full bg-primary/15 blur-3xl" />
        <div className="relative flex flex-col gap-6 p-6 sm:p-8 lg:flex-row lg:items-center lg:justify-between">
          <div className="max-w-2xl">
            <Badge variant="success" className="mb-3">
              <span className="size-1.5 rounded-full bg-success live-dot" /> Accepting miners now
            </Badge>
            <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">
              Point your rig at <span className="text-grad">katpool</span> in under two minutes
            </h2>
            <p className="mt-2 text-sm text-muted-foreground sm:text-base">
              Low {cfg.feePercent}% fee with a NACHO rebate, variable difficulty on every port, and a
              global anycast edge that routes you to the nearest server automatically.
            </p>
            <div className="mt-5 flex flex-wrap gap-2">
              <Badge variant="outline">
                <Gauge className="size-3.5" /> {cfg.feePercent}% fee + NACHO rebate
              </Badge>
              <Badge variant="outline">
                <Cpu className="size-3.5" /> Variable difficulty
              </Badge>
              <Badge variant="outline">
                <Globe2 className="size-3.5" /> {cfg.regions.length > 1 ? "7-region edge" : "Anycast edge"}
              </Badge>
              <Badge variant="outline">
                <Wallet className="size-3.5" /> {cfg.minPayoutKas} KAS min payout
              </Badge>
            </div>
          </div>
          <div className="flex shrink-0 flex-col gap-2 sm:flex-row lg:flex-col">
            <Button asChild size="lg">
              <a href="#connect">
                Connection settings <ArrowRight className="size-4" />
              </a>
            </Button>
            <Button asChild variant="outline" size="lg">
              <Link href="/leaders">View leaderboard</Link>
            </Button>
          </div>
        </div>
      </Card>

      {/* Steps */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        <Step n={1} icon={Wallet} title="Get a Kaspa address">
          You&apos;re paid directly to your own wallet — katpool never holds your coins. Create a{" "}
          {isTestnet ? (
            <>
              testnet wallet and fund it from the{" "}
              <a href="https://faucet-tn10.kaspanet.io" target="_blank" rel="noreferrer">
                tn10 faucet
              </a>
            </>
          ) : (
            <>
              wallet (e.g.{" "}
              <a href="https://kaspium.io" target="_blank" rel="noreferrer">
                Kaspium
              </a>{" "}
              or the Kaspa desktop wallet)
            </>
          )}
          , then copy your <span className="font-mono text-foreground">{cfg.addressPrefix}:</span>{" "}
          receiving address.
        </Step>
        <Step n={2} icon={Plug} title="Configure your miner">
          Set the pool URL, use your address (optionally <span className="font-mono">.worker</span>)
          as the username, and any value as the password. Full settings are{" "}
          <a href="#connect">below</a>.
        </Step>
        <Step n={3} icon={LineChart} title="Watch it live">
          Your rig appears within a minute. Paste your address into the search bar at the top — or
          find it on the <Link href="/leaders">leaderboard</Link> — to follow hashrate, workers,
          shares, balance and payouts in real time.
        </Step>
      </div>

      {/* Connection settings */}
      <div id="connect" className="scroll-mt-6 grid grid-cols-1 gap-6 lg:grid-cols-3">
        <Panel
          className="lg:col-span-2"
          eyebrow="Connect"
          title="Connection settings"
          description="Works with every kHeavyHash ASIC — IceRiver, Bitmain Antminer KS, and Goldshell."
        >
          <div className="space-y-3">
            <CopyField label="Stratum URL" value={stratumUrl} />
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <CopyField label="Username" value={userExample} />
              <CopyField label="Password" value="x" />
            </div>
            <p className="text-xs text-muted-foreground">
              Replace <span className="font-mono">your-wallet-address</span> with your real{" "}
              {cfg.addressPrefix}: address. The text after the dot
              (<span className="font-mono">.rig1</span>) is your worker name — pick anything. The
              password is ignored, so any value works.
            </p>

            <div className="space-y-2 pt-2">
              <p className="text-[0.6875rem] font-medium uppercase tracking-[0.12em] text-muted-foreground">
                On your ASIC
              </p>
              <p className="text-xs leading-relaxed text-muted-foreground">
                Open the miner&apos;s web dashboard, go to{" "}
                <span className="font-medium text-foreground">Settings → Pools</span>, and enter the
                values above as <span className="font-medium text-foreground">Pool 1</span> (URL,
                worker, password). Save — the rig reconnects to katpool automatically.
              </p>
              <div className="flex flex-wrap gap-1.5 pt-1">
                {ASIC_MODELS.map((model) => (
                  <Badge key={model} variant="outline">
                    {model}
                  </Badge>
                ))}
              </div>
            </div>
          </div>
        </Panel>

        <Panel eyebrow="Choose a port" title="Ports & starting difficulty" description="Vardiff tunes from here — any port is fine.">
          <div className="overflow-hidden rounded-xl border border-border">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border bg-muted/40 text-left text-xs text-muted-foreground">
                  <th className="px-4 py-2.5 font-medium">Port</th>
                  <th className="px-4 py-2.5 text-right font-medium">Start diff</th>
                  <th className="px-4 py-2.5 text-right font-medium" />
                </tr>
              </thead>
              <tbody>
                {cfg.ports.map((p) => (
                  <tr key={p.port} className="border-b border-border/60 last:border-0">
                    <td className="px-4 py-2.5 font-mono">{p.port}</td>
                    <td className="px-4 py-2.5 text-right tnum">{formatNumber(p.seed)}</td>
                    <td className="px-4 py-2.5 text-right">
                      <CopyButton value={`${primary.host}:${p.port}`} label={`Copy ${primary.host}:${p.port}`} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="mt-3 text-xs text-muted-foreground">
            Lower ports seed a lower difficulty (smaller rigs); higher ports seed higher. Because
            variable difficulty is on everywhere, the seed is only a starting point.
          </p>
        </Panel>
      </div>

      {/* Endpoints + fees */}
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <Panel
          eyebrow="Servers"
          title="Regional endpoints"
          description={
            cfg.regions.length > 1
              ? "Use the nearest host, or the global name to let anycast pick for you."
              : "Connect to the pool host below."
          }
        >
          <div className="divide-y divide-border/60">
            {cfg.regions.map((r) => (
              <div key={r.host} className="flex items-center justify-between gap-3 py-2.5">
                <div className="flex items-center gap-2">
                  <Globe2 className="size-4 text-muted-foreground" />
                  <span className="text-sm">{r.label}</span>
                  {r.primary ? <Badge variant="default">Recommended</Badge> : null}
                </div>
                <div className="flex items-center gap-1">
                  <span className="font-mono text-sm text-muted-foreground">{r.host}</span>
                  <CopyButton value={r.host} label={`Copy ${r.host}`} />
                </div>
              </div>
            ))}
          </div>
        </Panel>

        <Panel eyebrow="Economics" title="Fees & payouts" description="Transparent, miner-first economics.">
          <ul className="space-y-3 text-sm">
            <li className="flex items-start gap-3">
              <Gauge className="mt-0.5 size-4 shrink-0 text-primary" />
              <span>
                <span className="font-medium text-foreground">{cfg.feePercent}% topline fee</span> — among
                the lowest anywhere, taken only off block rewards you help find (PROP).
              </span>
            </li>
            <li className="flex items-start gap-3">
              <ShieldCheck className="mt-0.5 size-4 shrink-0 text-primary" />
              <span>
                <span className="font-medium text-foreground">NACHO rebate</span> — Standard miners get
                33% of the fee back as NACHO; Elite miners get 100%, paid automatically.
              </span>
            </li>
            <li className="flex items-start gap-3">
              <Wallet className="mt-0.5 size-4 shrink-0 text-primary" />
              <span>
                <span className="font-medium text-foreground">{cfg.minPayoutKas} KAS minimum</span> —
                automatic payouts run on a ~6-hour cycle straight to your wallet.
              </span>
            </li>
            <li className="flex items-start gap-3">
              <KeyRound className="mt-0.5 size-4 shrink-0 text-primary" />
              <span>
                <span className="font-medium text-foreground">Non-custodial</span> — rewards are sent to
                your address; the pool never holds miner funds.
              </span>
            </li>
          </ul>
        </Panel>
      </div>

      {/* FAQ */}
      <Panel eyebrow="Good to know" title="FAQ">
        <dl className="grid grid-cols-1 gap-x-8 gap-y-5 sm:grid-cols-2">
          <div>
            <dt className="text-sm font-medium text-foreground">Do I set a difficulty?</dt>
            <dd className="mt-1 text-sm text-muted-foreground">
              No. Variable difficulty adjusts automatically toward a steady share rate. The port you
              pick only sets the starting point.
            </dd>
          </div>
          <div>
            <dt className="text-sm font-medium text-foreground">Which miners are supported?</dt>
            <dd className="mt-1 text-sm text-muted-foreground">
              Every kHeavyHash ASIC — IceRiver KS-series, Bitmain Antminer KS-series, and Goldshell
              KA-series. Kaspa is ASIC-only; CPU and GPU mining is no longer competitive.
            </dd>
          </div>
          <div>
            <dt className="text-sm font-medium text-foreground">How are workers named?</dt>
            <dd className="mt-1 text-sm text-muted-foreground">
              Append <span className="font-mono">.name</span> to your address in the username (e.g.{" "}
              <span className="font-mono">{cfg.addressPrefix}:…​.rig1</span>) to track rigs separately.
            </dd>
          </div>
          <div>
            <dt className="text-sm font-medium text-foreground">Seeing rejects at first?</dt>
            <dd className="mt-1 text-sm text-muted-foreground">
              A few are normal while difficulty converges. If they persist, start on a higher-difficulty
              port for your hashrate.
            </dd>
          </div>
        </dl>
      </Panel>
    </div>
  );
}
