"use client";

import { useBookStore, type BookLevel } from "@/stores/bookStore";
import { formatPrice, formatAmount } from "@/lib/format";
import clsx from "clsx";

export function Orderbook() {
  const bids = useBookStore((s) => s.bids);
  const asks = useBookStore((s) => s.asks);
  const lastPrice = useBookStore((s) => s.lastPrice);

  const sortedBids = [...bids.values()].sort((a, b) => parseFloat(b.price) - parseFloat(a.price)).slice(0, 12);
  const sortedAsks = [...asks.values()].sort((a, b) => parseFloat(a.price) - parseFloat(b.price)).slice(0, 12);

  const maxQty = Math.max(
    ...sortedBids.map((l) => parseFloat(l.qty) || 0),
    ...sortedAsks.map((l) => parseFloat(l.qty) || 0),
    0.0001,
  );

  return (
    <section aria-label="Order book" className="rounded-xl border border-line bg-bg-surface">
      <header className="grid grid-cols-3 border-b border-line px-4 py-3 text-[11px] uppercase tracking-[0.14em] text-text-dim">
        <span>Price</span>
        <span className="text-right">Size</span>
        <span className="text-right">Total</span>
      </header>

      <div className="px-2 py-1">
        {sortedAsks.length === 0 && (
          <p className="px-3 py-4 text-center text-xs text-text-dim">No asks</p>
        )}
        {sortedAsks
          .slice()
          .reverse()
          .map((l) => (
            <Row key={`a-${l.price}`} level={l} side="ask" maxQty={maxQty} />
          ))}
      </div>

      <div
        aria-label="Last trade price"
        className="flex items-center justify-between border-y border-line bg-bg-raised px-4 py-2"
      >
        <span className="font-mono text-base font-medium text-text-primary">
          {lastPrice ? formatPrice(lastPrice) : "—"}
        </span>
        <span className="text-[11px] uppercase tracking-[0.14em] text-text-dim">last</span>
      </div>

      <div className="px-2 py-1">
        {sortedBids.length === 0 && (
          <p className="px-3 py-4 text-center text-xs text-text-dim">No bids</p>
        )}
        {sortedBids.map((l) => (
          <Row key={`b-${l.price}`} level={l} side="bid" maxQty={maxQty} />
        ))}
      </div>
    </section>
  );
}

function Row({ level, side, maxQty }: { level: BookLevel; side: "bid" | "ask"; maxQty: number }) {
  const qty = parseFloat(level.qty) || 0;
  const widthPct = Math.min(100, (qty / maxQty) * 100);
  const colorClass = side === "bid" ? "bg-emerald-500/8 text-emerald-400" : "bg-rose-500/8 text-rose-400";
  return (
    <div
      className={clsx(
        "relative grid grid-cols-3 px-2 py-1 font-mono text-sm",
        colorClass,
      )}
    >
      <div
        aria-hidden
        className={clsx(
          "absolute inset-y-0 right-0",
          side === "bid" ? "bg-emerald-500/8" : "bg-rose-500/8",
        )}
        style={{ width: `${widthPct}%` }}
      />
      <span className="relative">{formatPrice(level.price)}</span>
      <span className="relative text-right text-text-primary">{formatAmount(level.qty, 3)}</span>
      <span className="relative text-right text-text-muted">{formatAmount(String(qty * parseFloat(level.price)), 2)}</span>
    </div>
  );
}
