"use client";

import { useWallet } from "@solana/wallet-adapter-react";
import { WalletMultiButton } from "@solana/wallet-adapter-react-ui";
import { useEffect } from "react";
import bs58 from "bs58";
import { PublicKey } from "@solana/web3.js";

import { api } from "@/lib/api";
import { useSessionStore } from "@/stores/sessionStore";

/**
 * Connect wallet + Sign-In With Solana. The button itself is the official
 * `WalletMultiButton` from `@solana/wallet-adapter-react-ui`; we wire its
 * connection event into the SIWS handshake and stash the resulting JWT.
 */
export function WalletButton() {
  const { publicKey, signMessage, connected, disconnect } = useWallet();
  const setSession = useSessionStore((s) => s.set);
  const clearSession = useSessionStore((s) => s.clear);
  const jwt = useSessionStore((s) => s.jwt);

  useEffect(() => {
    if (!connected || !publicKey || !signMessage) return;

    let cancelled = false;
    (async () => {
      try {
        const { nonce, message } = await api.authNonce(publicKey.toBase58());
        const messageBytes = new TextEncoder().encode(message);
        const sigBytes = await signMessage(messageBytes);
        const signature = bs58.encode(sigBytes);
        if (cancelled) return;
        const { jwt: token, expires_at_unix_ms } = await api.authVerify(
          publicKey.toBase58(),
          nonce,
          signature,
        );
        if (cancelled) return;
        setSession({
          jwt: token,
          pubkey: new PublicKey(publicKey.toBase58()),
          expiresAtMs: expires_at_unix_ms,
        });
      } catch (e) {
        console.warn("sign-in failed:", e);
        disconnect().catch(() => {});
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [connected, publicKey, signMessage, setSession, disconnect]);

  useEffect(() => {
    if (!connected) clearSession();
  }, [connected, clearSession]);

  return (
    <div className="flex items-center gap-3" data-auth={jwt ? "signed-in" : "anonymous"}>
      <WalletMultiButton
        className="!h-9 !rounded-full !bg-bg-raised !px-4 !text-sm !font-medium !text-text-primary hover:!bg-line-strong"
        style={{ borderRadius: 9999 }}
      />
    </div>
  );
}
