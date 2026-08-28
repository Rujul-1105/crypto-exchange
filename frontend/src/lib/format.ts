import { PublicKey } from "@solana/web3.js";

/** Truncate a base58 pubkey for display: `4nE1...zXq9`. */
export function truncateAddress(address: string | PublicKey, head = 4, tail = 4): string {
  const s = typeof address === "string" ? address : address.toBase58();
  if (s.length <= head + tail + 1) return s;
  return `${s.slice(0, head)}...${s.slice(-tail)}`;
}

/** Format a decimal string with up to `decimals` trailing digits. */
export function formatAmount(value: string | number, decimals = 4): string {
  const n = typeof value === "string" ? Number(value) : value;
  if (!Number.isFinite(n)) return "0";
  return n.toLocaleString("en-US", {
    minimumFractionDigits: 0,
    maximumFractionDigits: decimals,
  });
}

/** Render a USD-style price with two decimals and grouping. */
export function formatPrice(value: string | number): string {
  const n = typeof value === "string" ? Number(value) : value;
  if (!Number.isFinite(n)) return "0.00";
  return n.toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
}
