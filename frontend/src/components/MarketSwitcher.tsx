"use client";

import { useQuery } from "@tanstack/react-query";

import { api } from "@/lib/api";
import { useUiStore } from "@/stores/uiStore";

export function MarketSwitcher() {
  const symbol = useUiStore((s) => s.symbol);
  const setSymbol = useUiStore((s) => s.setSymbol);

  const { data } = useQuery({
    queryKey: ["symbols"],
    queryFn: () => api.symbols(),
    staleTime: 60_000,
  });

  const symbols = data && data.length > 0 ? data : [symbol];

  return (
    <div className="inline-flex rounded-full bg-bg-raised p-1 text-xs">
      {symbols.map((s) => (
        <button
          key={s}
          onClick={() => setSymbol(s)}
          aria-pressed={s === symbol}
          className={`rounded-full px-3 py-1.5 font-mono transition ${
            s === symbol
              ? "bg-bg-surface text-text-primary"
              : "text-text-muted hover:text-text-primary"
          }`}
        >
          {s}
        </button>
      ))}
    </div>
  );
}
