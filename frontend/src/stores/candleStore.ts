"use client";

import { create } from "zustand";

export type Candle = {
  time: number; // unix seconds (lightweight-charts convention)
  open: number;
  high: number;
  low: number;
  close: number;
  volume?: number;
};

const RING_CAP = 1000;

type CandleState = {
  byInterval: Record<string, Candle[]>;
  update: (interval: string, c: Candle) => void;
  setSeries: (interval: string, candles: Candle[]) => void;
  clear: () => void;
};

export const useCandleStore = create<CandleState>((set) => ({
  byInterval: {},
  update: (interval, c) =>
    set((s) => {
      const existing = s.byInterval[interval] ?? [];
      if (existing.length === 0) {
        return { byInterval: { ...s.byInterval, [interval]: [c] } };
      }
      const last = existing[existing.length - 1];
      const merged =
        last.time === c.time
          ? [...existing.slice(0, -1), c]
          : [...existing, c];
      if (merged.length > RING_CAP) merged.splice(0, merged.length - RING_CAP);
      return { byInterval: { ...s.byInterval, [interval]: merged } };
    }),
  setSeries: (interval, candles) =>
    set((s) => ({ byInterval: { ...s.byInterval, [interval]: candles } })),
  clear: () => set({ byInterval: {} }),
}));
