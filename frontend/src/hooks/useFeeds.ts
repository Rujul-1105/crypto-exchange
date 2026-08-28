"use client";

import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";

import { api, type Trade } from "@/lib/api";
import { useBookStore, type BookLevel } from "@/stores/bookStore";
import { useTradeStore } from "@/stores/tradeStore";
import { useWebSocket } from "@/hooks/useWebSocket";

/** Subscribes to `book:<sym>` and `trades:<sym>` via WS, hydrates from REST. */
export function useMarketFeeds(symbol: string) {
  const wsBase = (process.env.NEXT_PUBLIC_WS_URL ?? "ws://localhost:8080/ws");
  const bookApplySnapshot = useBookStore((s) => s.applySnapshot);
  const bookApplyDelta = useBookStore((s) => s.applyDelta);
  const bookReset = useBookStore((s) => s.reset);
  const tradePush = useTradeStore((s) => s.push);
  const tradeClear = useTradeStore((s) => s.clear);

  const depthQuery = useQuery({
    queryKey: ["depth", symbol],
    queryFn: () => api.depth(symbol, 20),
    enabled: !!symbol,
  });

  const tradesQuery = useQuery({
    queryKey: ["trades", symbol],
    queryFn: () => api.trades(symbol, 50),
    enabled: !!symbol,
  });

  useEffect(() => {
    bookReset();
    tradeClear();
    if (depthQuery.data) {
      bookApplySnapshot(depthQuery.data.bids, depthQuery.data.asks, depthQuery.data.last_trade_price);
    }
  }, [symbol, depthQuery.data, bookApplySnapshot, bookReset, tradeClear]);

  useEffect(() => {
    if (!tradesQuery.data) return;
    tradeClear();
    for (const t of [...tradesQuery.data].reverse()) {
      tradePush({
        id: t.id,
        price: t.price,
        qty: t.quantity,
        taker_side: t.taker_side,
        ts: t.ts * 1000,
      });
    }
  }, [tradesQuery.data, tradePush, tradeClear]);

  const { subscribe, unsubscribe } = useWebSocket(wsBase, (msg) => {
    if (!msg || typeof msg !== "object") return;
    const event = (msg as any).event;
    const channel = (msg as any).channel;
    if (!event || !channel) return;
    if (channel === `book:${symbol}`) {
      if (event.type === "snapshot") {
        bookApplySnapshot(event.bids as BookLevel[], event.asks as BookLevel[], event.last_trade_price ?? null);
      } else if (event.type === "delta") {
        bookApplyDelta(event.changes ?? []);
      }
    } else if (channel === `trades:${symbol}`) {
      if (event.type === "trade") {
        const t = event.trade as Trade;
        tradePush({
          id: t.id,
          price: t.price,
          qty: t.quantity,
          taker_side: t.taker_side,
          ts: (t.ts ?? Date.now() / 1000) * 1000,
        });
      }
    }
  });

  useEffect(() => {
    subscribe(`book:${symbol}`);
    subscribe(`trades:${symbol}`);
    return () => {
      unsubscribe(`book:${symbol}`);
      unsubscribe(`trades:${symbol}`);
    };
  }, [symbol, subscribe, unsubscribe]);
}
