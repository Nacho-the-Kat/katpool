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
  title: "Kat Pool — Kaspa Mining Pool",
  description:
    "Open-source Kaspa mining pool with global anycast stratum, PROP payouts, and NACHO fee rebates. Start mining at the edge.",
  icons: {
    icon: "/katpool-icon.png",
    apple: "/icon-192.png",
  },
  openGraph: {
    title: "Kat Pool — Kaspa Mining Pool",
    description: "Mine Kaspa at the edge. Global stratum, transparent PROP payouts, NACHO rebates.",
    siteName: "Kat Pool",
    type: "website",
  },
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
