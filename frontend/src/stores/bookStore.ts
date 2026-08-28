"use client";

import { create } from "zustand";

type Side = "buy" | "sell";
export type BookLevel = { price: string; qty: string };

type BookState = {
  bids: Map<number, BookLevel>;
  asks: Map<number, BookLevel>;
  lastPrice: string | null;
  ready: boolean;

  applySnapshot: (bids: BookLevel[], asks: BookLevel[], lastPrice: string | null) => void;
  applyDelta: (changes: { side: Side; price: string; qty: string }[]) => void;
  reset: () => void;
};

function key(price: string): number {
  // Stable map key from decimal-string price.
  return parseFloat(price) * 1e9;
}

export const useBookStore = create<BookState>((set) => ({
  bids: new Map(),
  asks: new Map(),
  lastPrice: null,
  ready: false,

  applySnapshot: (bids, asks, lastPrice) => {
    const b = new Map<number, BookLevel>();
    bids.forEach((l) => b.set(key(l.price), l));
    const a = new Map<number, BookLevel>();
    asks.forEach((l) => a.set(key(l.price), l));
    set({ bids: b, asks: a, lastPrice, ready: true });
  },

  applyDelta: (changes) => {
    set((state) => {
      const bids = new Map(state.bids);
      const asks = new Map(state.asks);
      for (const c of changes) {
        const map = c.side === "buy" ? bids : asks;
        const k = key(c.price);
        const qty = parseFloat(c.qty);
        if (!Number.isFinite(qty) || qty <= 0) {
          map.delete(k);
        } else {
          map.set(k, { price: c.price, qty: c.qty });
        }
      }
      return { bids, asks };
    });
  },

  reset: () => set({ bids: new Map(), asks: new Map(), lastPrice: null, ready: false }),
}));
