"use client";

/**
 * MarketHeader — Backpack-style summary row that sits above the candle chart.
 *
 * Layout (single row, three zones):
 *   ┌─────────────────────────────────────────────────────────────────────┐
 *   │ SOL/USDC [Spot]   103.15  +1.63 +1.61%     1H HIGH  1H LOW         │
 *   │                                  (live, color-coded)   103.92  101.40│
 *   └─────────────────────────────────────────────────────────────────────┘
 *
 * Live price comes from `bookStore.lastPrice` (driven by `useMarketFeeds`).
 * High/low/change are derived from the live 1h candle in `candleStore` —
 * for the demo we label them "1H" since there's no 24h aggregator on the
 * backend. The price color flips between `accent.buy` (green) and
 * `accent.sell` (red) based on change vs. the 1h candle's open.
 */

import { useBookStore } from "@/stores/bookStore";
import { useCandleStore } from "@/stores/candleStore";
import { formatPrice } from "@/lib/format";

const SPOT_LABEL = "Spot";

// Module-scope sentinel — keeps the selector stable when `byInterval["1h"]`
// is `undefined` (no 1h candle has been produced yet). A new `[]` from
// `?? []` on each render would cause Zustand to re-render the component
// every time any store updates.
const EMPTY_CANDLES: { time: number; open: number; high: number; low: number; close: number; volume?: number }[] = [];

export function MarketHeader({ symbol }: { symbol: string }) {
  const lastPrice = useBookStore((s) => s.lastPrice);
  const hourCandles = useCandleStore((s) => s.byInterval["1h"] ?? EMPTY_CANDLES);
  // The most-recent 1h candle is the live, in-progress bucket.
  const last1h = hourCandles.length > 0 ? hourCandles[hourCandles.length - 1] : null;

  // Prefer the live trade price from the book; fall back to the 1h close so
  // the header isn't blank during the first render before WS connects.
  const livePrice = lastPrice ?? (last1h ? last1h.close.toString() : null);

  const change = last1h ? last1h.close - last1h.open : 0;
  const changePct = last1h && last1h.open !== 0 ? (change / last1h.open) * 100 : 0;
  const positive = change >= 0;
  const color = positive ? "text-accent-buy" : "text-accent-sell";

  const displaySymbol = symbol.replace("-", "/");

  return (
    <section
      aria-label="Market summary"
      className="rounded-xl border border-line bg-bg-surface px-4 py-3 md:px-6"
    >
      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        {/* Symbol + Spot badge */}
        <div className="flex items-center gap-3">
          <span className="font-mono text-2xl font-semibold tracking-tight text-text-primary">
            {displaySymbol}
          </span>
          <span className="rounded-md bg-bg-raised px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-text-muted">
            {SPOT_LABEL}
          </span>
        </div>

        {/* Live price + change */}
        <div className="flex items-baseline gap-3">
          <span className={`font-mono text-3xl font-semibold tabular-nums ${color}`}>
            {livePrice !== null ? formatPrice(livePrice) : "—"}
          </span>
          {last1h && (
            <span className={`font-mono text-sm tabular-nums ${color}`}>
              {positive ? "+" : ""}
              {formatPrice(change.toString())} {positive ? "+" : ""}
              {changePct.toFixed(2)}%
            </span>
          )}
        </div>

        {/* 1H High / Low metrics */}
        <div className="ml-auto flex items-center gap-6">
          <Stat
            label="1H HIGH"
            value={last1h ? formatPrice(last1h.high.toString()) : "—"}
          />
          <Stat
            label="1H LOW"
            value={last1h ? formatPrice(last1h.low.toString()) : "—"}
          />
        </div>
      </div>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col">
      <span className="text-[10px] uppercase tracking-[0.18em] text-text-dim">
        {label}
      </span>
      <span className="font-mono text-sm tabular-nums text-text-primary">
        {value}
      </span>
    </div>
  );
}