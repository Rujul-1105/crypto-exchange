"use client";

import { useParams } from "next/navigation";
import { useEffect } from "react";

import { WalletButton } from "@/components/WalletButton";
import { MarketSwitcher } from "@/components/MarketSwitcher";
import { Orderbook } from "@/components/Orderbook";
import { TradeTape } from "@/components/TradeTape";
import { OrderForm } from "@/components/OrderForm";
import { CandleChart } from "@/components/CandleChart";
import { useMarketFeeds } from "@/hooks/useFeeds";
import { useUiStore } from "@/stores/uiStore";

export default function TradePage() {
  const params = useParams<{ symbol: string }>();
  const symbol = useUiStore((s) => s.symbol);
  const setSymbol = useUiStore((s) => s.setSymbol);

  useEffect(() => {
    if (params.symbol && params.symbol !== symbol) setSymbol(params.symbol);
  }, [params.symbol, symbol, setSymbol]);

  useMarketFeeds(symbol);

  return (
    <main className="mx-auto max-w-7xl px-4 py-6 md:px-8 md:py-10">
      <header className="mb-8 flex flex-wrap items-center justify-between gap-4">
        <div>
          <p className="text-xs uppercase tracking-[0.18em] text-text-dim">CEX Demo</p>
          <h1 className="mt-1 font-mono text-2xl font-semibold tracking-tight text-text-primary">
            {symbol}
          </h1>
        </div>
        <div className="flex items-center gap-3">
          <MarketSwitcher />
          <WalletButton />
        </div>
      </header>

      <div className="grid grid-cols-1 gap-4 md:grid-cols-[1fr_320px] lg:grid-cols-[1fr_360px_300px]">
        <div className="space-y-4">
          <CandleChart symbol={symbol} interval="1m" />
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <Orderbook />
            <TradeTape />
          </div>
        </div>

        <OrderForm />
      </div>
    </main>
  );
}
