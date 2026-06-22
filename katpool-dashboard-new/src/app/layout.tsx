import type { Metadata, Viewport } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { Providers } from "@/components/providers";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AppShell } from "@/components/shell/app-shell";

const geistSans = Geist({ variable: "--font-geist-sans", subsets: ["latin"] });
const geistMono = Geist_Mono({ variable: "--font-geist-mono", subsets: ["latin"] });

const POOL_NAME = process.env.NEXT_PUBLIC_POOL_NAME ?? "katpool";
const SITE_URL = process.env.NEXT_PUBLIC_SITE_URL ?? "https://katpool.com";

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: `${POOL_NAME} — Kaspa Mining Pool`,
    template: `%s · ${POOL_NAME}`,
  },
  description:
    "Real-time hashrate, blocks, payouts and per-miner analytics for the katpool Kaspa mining pool.",
  applicationName: POOL_NAME,
  keywords: [
    "Kaspa",
    "KAS",
    "mining pool",
    "katpool",
    "NACHO",
    "stratum",
    "hashrate",
    "crypto mining",
  ],
  authors: [{ name: "Kat Pool" }],
  creator: "Kat Pool",
  openGraph: {
    title: `${POOL_NAME} — Kaspa Mining Pool`,
    description: "Real-time pool analytics: hashrate, blocks, payouts, and miner insights.",
    siteName: POOL_NAME,
    type: "website",
    url: "/",
    locale: "en_US",
  },
  twitter: {
    card: "summary_large_image",
    title: `${POOL_NAME} — Kaspa Mining Pool`,
    description: "Real-time pool analytics: hashrate, blocks, payouts, and miner insights.",
    creator: "@katpool",
  },
  robots: { index: true, follow: true },
  alternates: { canonical: "/" },
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: dark)", color: "#0b0e12" },
    { media: "(prefers-color-scheme: light)", color: "#fafbfc" },
  ],
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className={`${geistSans.variable} ${geistMono.variable} font-sans antialiased`}>
        <Providers>
          <TooltipProvider delayDuration={150}>
            <AppShell>{children}</AppShell>
          </TooltipProvider>
        </Providers>
      </body>
    </html>
  );
}
