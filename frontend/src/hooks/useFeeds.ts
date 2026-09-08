"use client";

import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";

import { api, type Trade } from "@/lib/api";
import { useBookStore } from "@/stores/bookStore";
import { useTradeStore } from "@/stores/tradeStore";
import { useWebSocket } from "@/hooks/useWebSocket";

/** Subscribes to `book:<sym>` and `trades:<sym>` via WS, hydrates from REST. */
export function useMarketFeeds(symbol: string) {
  const wsBase = process.env.NEXT_PUBLIC_WS_URL ?? "ws://localhost:8080/ws";
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
      // REST depth rows are tuples [price, qty] — flatten to the BookLevel shape.
      const bids = depthQuery.data.bids.map(([price, qty]) => ({ price, qty }));
      const asks = depthQuery.data.asks.map(([price, qty]) => ({ price, qty }));
      bookApplySnapshot(bids, asks, depthQuery.data.last_trade_price);
    }
  }, [symbol, depthQuery.data, bookApplySnapshot, bookReset, tradeClear]);

  useEffect(() => {
    if (!tradesQuery.data) return;
    tradeClear();
    // REST returns the newest trade last; reverse so we push oldest-first
    // and the ring ends up newest-first.
    for (const t of [...tradesQuery.data].reverse()) {
      tradePush({
        id: t.id,
        price: t.price,
        qty: t.quantity,
        taker_side: t.taker_side,
        ts: t.timestamp,
      });
    }
  }, [tradesQuery.data, tradePush, tradeClear]);

  const { subscribe, unsubscribe } = useWebSocket(wsBase, (msg) => {
    if (!msg || typeof msg !== "object") return;
    const event = (msg as any).event;
    const channel = (msg as any).channel;
    if (!event || !channel) return;

    if (channel === `book:${symbol}`) {
      // Backend `EngineEvent` is `#[serde(tag="type", rename_all="snake_case")],
      // so individual book changes arrive as `BookDelta` events (one per
      // level change). Map each into the store's delta shape: a single
      // `{side, price, qty}` change with `qty === "0"` meaning remove level.
      if (event.type === "book_delta") {
        bookApplyDelta([
          {
            side: event.side,
            price: String(event.price),
            qty: String(event.new_qty ?? "0"),
          },
        ]);
      }
    } else if (channel === `trades:${symbol}`) {
      if (event.type === "trade") {
        const t = event.trade as Trade;
        tradePush({
          id: t.id,
          price: t.price,
          qty: t.quantity,
          taker_side: t.taker_side,
          ts: t.timestamp ?? Date.now(),
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