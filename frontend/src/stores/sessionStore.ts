"use client";

import { create } from "zustand";
import { PublicKey } from "@solana/web3.js";

type SessionState = {
  jwt: string | null;
  userPubkey: PublicKey | null;
  expiresAtMs: number | null;
  set: (s: { jwt: string; pubkey: PublicKey; expiresAtMs: number }) => void;
  clear: () => void;
};

export const useSessionStore = create<SessionState>((set) => ({
  jwt: null,
  userPubkey: null,
  expiresAtMs: null,
  set: ({ jwt, pubkey, expiresAtMs }) => set({ jwt, userPubkey: pubkey, expiresAtMs }),
  clear: () => set({ jwt: null, userPubkey: null, expiresAtMs: null }),
}));
