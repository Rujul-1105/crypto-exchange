"use client";

import { create } from "zustand";

export type Trade = {
  id: number;
  price: string;
  qty: string;
  taker_side: "buy" | "sell";
  ts: number;
};

const RING_CAP = 100;

type TradeState = {
  trades: Trade[];
  push: (t: Trade) => void;
  clear: () => void;
};

export const useTradeStore = create<TradeState>((set) => ({
  trades: [],
  push: (t) =>
    set((s) => {
      const next = [t, ...s.trades];
      if (next.length > RING_CAP) next.length = RING_CAP;
      return { trades: next };
    }),
  clear: () => set({ trades: [] }),
}));
