import type { Metadata, Viewport } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  metadataBase: new URL(process.env.NEXT_PUBLIC_SITE_URL ?? "https://katpool.com"),
  title: "Kat Pool - Kaspa Mining Pool",
  description:
    "Open-source Kaspa mining pool with global anycast stratum, PROP payouts, and NACHO fee rebates. Start mining at the edge.",
  keywords: [
    "Kaspa",
    "KAS",
    "mining pool",
    "katpool",
    "NACHO",
    "stratum",
    "PROP",
    "crypto mining",
  ],
  authors: [{ name: "Kat Pool" }],
  creator: "Kat Pool",
  icons: {
    icon: "/katpool-icon.png",
    apple: "/icon-192.png",
  },
  openGraph: {
    title: "Kat Pool - Kaspa Mining Pool",
    description: "Mine Kaspa at the edge. Global stratum, transparent PROP payouts, NACHO rebates.",
    siteName: "Kat Pool",
    type: "website",
    locale: "en_US",
    url: "/",
  },
  twitter: {
    card: "summary_large_image",
    title: "Kat Pool - Kaspa Mining Pool",
    description: "Mine Kaspa at the edge. Global stratum, PROP payouts, NACHO rebates.",
    creator: "@katpool",
  },
  robots: { index: true, follow: true },
  alternates: { canonical: "/" },
};

export const viewport: Viewport = {
  themeColor: "#060e11",
  colorScheme: "dark",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className="dark">
      <body className={`${geistSans.variable} ${geistMono.variable} font-sans antialiased`}>
        {children}
      </body>
    </html>
  );
}
