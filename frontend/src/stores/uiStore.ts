"use client";

import { create } from "zustand";

type UiState = {
  symbol: string;
  setSymbol: (s: string) => void;
  interval: "1m" | "5m" | "1h";
  setInterval: (i: "1m" | "5m" | "1h") => void;
  orderSide: "buy" | "sell";
  setOrderSide: (s: "buy" | "sell") => void;
};

export const useUiStore = create<UiState>((set) => ({
  symbol: "SOL-USDC",
  setSymbol: (symbol) => set({ symbol }),
  interval: "1m",
  setInterval: (interval) => set({ interval }),
  orderSide: "buy",
  setOrderSide: (orderSide) => set({ orderSide }),
}));
