"use client";

import { AnchorProvider, Program, Idl } from "@coral-xyz/anchor";
import { AnchorWallet, useAnchorWallet, useConnection } from "@solana/wallet-adapter-react";
import { PublicKey } from "@solana/web3.js";
import { useMemo } from "react";

import idlJson from "../idl/exchange.json";

const PROGRAM_ID = process.env.NEXT_PUBLIC_PROGRAM_ID ?? "";
const SOL_MINT = new PublicKey(
  process.env.NEXT_PUBLIC_SOL_MINT ?? "So11111111111111111111111111111111111111112",
);
const USDC_MINT = new PublicKey(
  process.env.NEXT_PUBLIC_USDC_MINT ?? "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU",
);
const ATA_PROGRAM = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const TOKEN_PROGRAM = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

export const MINTS = { SOL: SOL_MINT, USDC: USDC_MINT } as const;

/** PDA that owns every user's vault token accounts. */
export function deriveVaultAuthorityPda(programId: PublicKey): PublicKey {
  const [pda] = PublicKey.findProgramAddressSync([Buffer.from("vault_authority")], programId);
  return pda;
}

/** User's vault token account (ATA of vault_authority on the given mint). */
export function deriveUserVaultAta(vaultAuthority: PublicKey, mint: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [vaultAuthority.toBuffer(), TOKEN_PROGRAM.toBuffer(), mint.toBuffer()],
    ATA_PROGRAM,
  )[0];
}

/** User's own ATA for the mint (used as the funding source on deposit, withdraw target). */
export function deriveUserAta(owner: PublicKey, mint: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM.toBuffer(), mint.toBuffer()],
    ATA_PROGRAM,
  )[0];
}

/** Per-user ledger PDA seeded by `[b"user_balance", user]`. Holds the user's
 *  available SOL and USDC balances after deposit / withdraw / settle_fill. */
export function deriveUserBalancePda(programId: PublicKey, user: PublicKey): PublicKey {
  const [pda] = PublicKey.findProgramAddressSync(
    [Buffer.from("user_balance"), user.toBuffer()],
    programId,
  );
  return pda;
}

/** Hook returning the Anchor Program (or null until the wallet + IDL are ready). */
export function useExchangeProgram(): Program | null {
  const { connection } = useConnection();
  const wallet = useAnchorWallet();

  return useMemo(() => {
    if (!wallet || !PROGRAM_ID) return null;
    try {
      const programId = new PublicKey(PROGRAM_ID);
      const provider = new AnchorProvider(connection, wallet as AnchorWallet, {
        commitment: "confirmed",
        preflightCommitment: "confirmed",
      });
      return new Program(idlJson as Idl, provider);
    } catch (e) {
      console.warn("exchange program init failed:", e);
      return null;
    }
  }, [connection, wallet]);
}
