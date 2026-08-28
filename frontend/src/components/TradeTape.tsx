"use client";

import { useTradeStore } from "@/stores/tradeStore";
import { formatPrice, formatAmount } from "@/lib/format";
import clsx from "clsx";

export function TradeTape() {
  const trades = useTradeStore((s) => s.trades);

  return (
    <section aria-label="Recent trades" className="rounded-xl border border-line bg-bg-surface">
      <header className="grid grid-cols-3 border-b border-line px-4 py-3 text-[11px] uppercase tracking-[0.14em] text-text-dim">
        <span>Price</span>
        <span className="text-right">Size</span>
        <span className="text-right">Time</span>
      </header>
      <ul className="divide-y divide-line">
        {trades.length === 0 && (
          <li className="px-4 py-6 text-center text-xs text-text-dim">Waiting for trades.</li>
        )}
        {trades.slice(0, 30).map((t) => (
          <li key={t.id} className="grid grid-cols-3 px-4 py-1.5 font-mono text-xs">
            <span
              className={clsx(
                t.taker_side === "buy" ? "text-emerald-400" : "text-rose-400",
              )}
            >
              {formatPrice(t.price)}
            </span>
            <span className="text-right text-text-primary">{formatAmount(t.qty, 3)}</span>
            <span className="text-right text-text-dim">{formatTime(t.ts)}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function formatTime(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString("en-US", { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
}
