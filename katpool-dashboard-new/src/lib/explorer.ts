const BASE = (
  process.env.NEXT_PUBLIC_EXPLORER_BASE_URL ?? "https://explorer.kaspa.org"
).replace(/\/+$/, "");

/** Explorer deep-link for an address. */
export function explorerAddress(address: string): string {
  return `${BASE}/addresses/${encodeURIComponent(address)}`;
}

/** Explorer deep-link for a block hash. */
export function explorerBlock(hash: string): string {
  return `${BASE}/blocks/${encodeURIComponent(hash)}`;
}

/** Explorer deep-link for a transaction hash. */
export function explorerTx(hash: string): string {
  return `${BASE}/txs/${encodeURIComponent(hash)}`;
}
