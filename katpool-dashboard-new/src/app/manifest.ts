import type { MetadataRoute } from "next";

const POOL_NAME = process.env.NEXT_PUBLIC_POOL_NAME ?? "katpool";

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: `${POOL_NAME} — Kaspa Mining Pool`,
    short_name: POOL_NAME,
    description:
      "Real-time hashrate, blocks, payouts and per-miner analytics for the katpool Kaspa mining pool.",
    start_url: "/",
    display: "standalone",
    background_color: "#0b0e12",
    theme_color: "#0b0e12",
    icons: [
      { src: "/icon-192.png", sizes: "192x192", type: "image/png", purpose: "any" },
      { src: "/icon-512.png", sizes: "512x512", type: "image/png", purpose: "any" },
      { src: "/icon-512-maskable.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
    ],
  };
}
