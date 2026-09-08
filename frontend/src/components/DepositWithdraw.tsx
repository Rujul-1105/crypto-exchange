"use client";

import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { PublicKey, SystemProgram, Transaction } from "@solana/web3.js";
import { BN } from "@coral-xyz/anchor";
import {
  createAssociatedTokenAccountInstruction,
  createSyncNativeInstruction,
  NATIVE_MINT,
} from "@solana/spl-token";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { ArrowDown, ArrowUp, ArrowsLeftRight } from "@phosphor-icons/react";

import { MINTS, deriveUserAta, deriveUserBalancePda, deriveUserVaultAta, deriveVaultAuthorityPda, useExchangeProgram } from "@/lib/anchorClient";

type Mode = "deposit" | "withdraw";
type Asset = "SOL" | "USDC";

const MINTS_BY_ASSET: Record<Asset, PublicKey> = {
  SOL: MINTS.SOL,
  USDC: MINTS.USDC,
};

const DECIMALS_BY_ASSET: Record<Asset, number> = { SOL: 9, USDC: 6 };

function toBaseUnits(amount: string, decimals: number): bigint | null {
  if (!amount) return null;
  const trimmed = amount.trim();
  if (!/^\d+(\.\d+)?$/.test(trimmed)) return null;
  const [whole, frac = ""] = trimmed.split(".");
  const padded = (frac + "0".repeat(decimals)).slice(0, decimals);
  try {
    return BigInt(whole) * BigInt(10) ** BigInt(decimals) + BigInt(padded || "0");
  } catch {
    return null;
  }
}

/** Wrap a BigInt as a BN for Anchor's borsh serializer. The u64 instruction
 *  args go through `@coral-xyz/borsh`'s BNLayout.encode which calls
 *  `src.toArrayLike(...)` — that method only exists on BN, not BigInt, so
 *  passing a raw BigInt throws `src.toArrayLike is not a function`. */
function toBN(baseUnits: bigint | null): BN | null {
  return baseUnits === null ? null : new BN(baseUnits.toString());
}

export function DepositWithdraw() {
  const { connection } = useConnection();
  const { publicKey, sendTransaction } = useWallet();
  const program = useExchangeProgram();
  const queryClient = useQueryClient();

  const [mode, setMode] = useState<Mode>("deposit");
  const [asset, setAsset] = useState<Asset>("SOL");
  const [amount, setAmount] = useState("");
  const [helper, setHelper] = useState<{ kind: "error" | "info"; text: string } | null>(null);

  const decimals = DECIMALS_BY_ASSET[asset];
  const baseUnits = useMemo(() => toBaseUnits(amount, decimals), [amount, decimals]);

  const mutation = useMutation({
    mutationFn: async () => {
      if (!publicKey || !program) throw new Error("wallet not ready");
      if (baseUnits === null || baseUnits <= 0n) throw new Error("Enter a positive amount");

      const programId = program.programId;
      const mint = MINTS_BY_ASSET[asset];
      const vaultAuthority = deriveVaultAuthorityPda(programId);
      const userVault = deriveUserVaultAta(vaultAuthority, mint);
      const userAta = deriveUserAta(publicKey, mint);

      const method = mode === "deposit"
        ? asset === "SOL" ? program.methods.depositSol(toBN(baseUnits)) : program.methods.depositUsdc(toBN(baseUnits))
        : asset === "SOL" ? program.methods.withdrawSol(toBN(baseUnits)) : program.methods.withdrawUsdc(toBN(baseUnits));

      // For deposit, ensure the user has an ATA for the mint first.
      if (mode === "deposit") {
        const ataInfo = await connection.getAccountInfo(userAta);
        if (!ataInfo) {
          const createAtaIx = createAssociatedTokenAccountInstruction(
            publicKey,
            userAta,
            publicKey,
            mint,
          );
          const tx = new Transaction().add(createAtaIx);
          const sig = await sendTransaction(tx, connection);
          await connection.confirmTransaction(sig, "confirmed");
        }

        // For SOL deposits, wrap any deficit from native SOL into wSOL. The
        // program expects wSOL (not native SOL) in the user's ATA; without
        // this step the token::transfer CPI in deposit_sol fails with
        // Token-program error 0x1 ("insufficient funds"). For USDC there's
        // no native equivalent, so we assume the user already holds USDC.
        if (asset === "SOL") {
          const lamportsNeeded = Number(baseUnits);
          const wsolAtaInfo = await connection.getAccountInfo(userAta);
          let currentWsolLamports = 0;
          if (wsolAtaInfo) {
            const view = new DataView(
              wsolAtaInfo.data.buffer,
              wsolAtaInfo.data.byteOffset,
              wsolAtaInfo.data.byteLength,
            );
            const low = view.getUint32(64, true);
            const high = view.getUint32(68, true);
            currentWsolLamports = low + high * 2 ** 32;
          }
          const deficit = lamportsNeeded - currentWsolLamports;
          if (deficit > 0) {
            // Wrap native SOL → wSOL by transferring lamports directly into
            // the user's wSOL ATA, then calling sync_native to update the
            // SPL token balance to match. The destination must be the wSOL
            // ATA itself (not the wSOL mint) — sync_native reads that
            // account's lamport balance. Two instructions in one tx.
            const wrapTx = new Transaction()
              .add(
                SystemProgram.transfer({
                  fromPubkey: publicKey,
                  toPubkey: userAta,
                  lamports: deficit,
                }),
              )
              .add(createSyncNativeInstruction(userAta));
            const wrapSig = await sendTransaction(wrapTx, connection);
            await connection.confirmTransaction(wrapSig, "confirmed");
          }
        }
      }

      const sig = await method
        .accounts({
          user: publicKey,
          config: deriveConfigPda(programId),
          vaultAuthority,
          ...(mode === "deposit"
            ? asset === "SOL"
              ? { userBalance: deriveUserBalancePda(programId, publicKey), sharedSolVault: userVault, userWsolAta: userAta, wsolMint: mint, tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ATA_PROGRAM_ID, systemProgram: SYSTEM_PROGRAM_ID }
              : { userBalance: deriveUserBalancePda(programId, publicKey), sharedUsdcVault: userVault, userUsdcAta: userAta, usdcMint: mint, tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ATA_PROGRAM_ID, systemProgram: SYSTEM_PROGRAM_ID }
            : asset === "SOL"
              ? { userBalance: deriveUserBalancePda(programId, publicKey), sharedSolVault: userVault, userWsolAta: userAta, wsolMint: mint, tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ATA_PROGRAM_ID, systemProgram: SYSTEM_PROGRAM_ID }
              : { userBalance: deriveUserBalancePda(programId, publicKey), sharedUsdcVault: userVault, userUsdcAta: userAta, usdcMint: mint, tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ATA_PROGRAM_ID, systemProgram: SYSTEM_PROGRAM_ID }),
        })
        .rpc();

      return sig;
    },
    onSuccess: () => {
      setHelper({ kind: "info", text: `${mode === "deposit" ? "Deposit" : "Withdraw"} confirmed.` });
      setAmount("");
      queryClient.invalidateQueries({ queryKey: ["balances"] });
    },
    onError: (e: Error) => {
      setHelper({ kind: "error", text: e.message });
    },
  });

  if (!publicKey) {
    return (
      <section className="rounded-xl border border-line bg-bg-surface p-6">
        <p className="text-sm text-text-muted">Connect a wallet to deposit or withdraw.</p>
      </section>
    );
  }

  return (
    <section className="rounded-xl border border-line bg-bg-surface">
      <header className="border-b border-line px-5 py-4">
        <h2 className="text-sm font-medium text-text-primary">Vault transfer</h2>
      </header>

      <div className="space-y-5 p-5">
        <div className="flex items-center gap-2">
          <ModeButton current={mode} target="deposit" onSelect={setMode} icon={<ArrowDown size={14} weight="bold" />}>
            Deposit
          </ModeButton>
          <ModeButton current={mode} target="withdraw" onSelect={setMode} icon={<ArrowUp size={14} weight="bold" />}>
            Withdraw
          </ModeButton>
          <div className="ml-auto inline-flex rounded-full bg-bg-raised p-1 text-xs">
            {(["SOL", "USDC"] as Asset[]).map((a) => (
              <button
                key={a}
                onClick={() => setAsset(a)}
                className={`rounded-full px-3 py-1 transition ${
                  asset === a ? "bg-bg-surface text-text-primary" : "text-text-muted hover:text-text-primary"
                }`}
              >
                {a}
              </button>
            ))}
          </div>
        </div>

        <label className="block">
          <span className="sr-only">Amount in {asset}</span>
          <div className="flex items-baseline gap-2 rounded-lg border border-line bg-bg-base px-4 py-3 focus-within:border-line-strong">
            <input
              inputMode="decimal"
              placeholder="0.00"
              value={amount}
              onChange={(e) => {
                setAmount(e.target.value);
                if (helper) setHelper(null);
              }}
              className="w-full bg-transparent font-mono text-lg text-text-primary outline-none placeholder:text-text-dim"
            />
            <span className="font-mono text-sm text-text-muted">{asset}</span>
          </div>
          {helper && (
            <p className={`mt-2 text-xs ${helper.kind === "error" ? "text-rose-400" : "text-emerald-400"}`}>
              {helper.text}
            </p>
          )}
        </label>

        <button
          onClick={() => mutation.mutate()}
          disabled={mutation.isPending || baseUnits === null || baseUnits <= 0n}
          className="flex w-full items-center justify-center gap-2 rounded-full bg-text-primary px-5 py-3 text-sm font-medium text-bg-base transition hover:-translate-y-px hover:bg-white active:translate-y-0 active:scale-[0.99] disabled:cursor-not-allowed disabled:bg-line disabled:text-text-muted disabled:hover:translate-y-0"
        >
          <ArrowsLeftRight size={16} weight="bold" />
          {mutation.isPending
            ? "Confirming..."
            : `${mode === "deposit" ? "Deposit" : "Withdraw"} ${asset}`}
        </button>
      </div>
    </section>
  );
}

function ModeButton({
  current,
  target,
  onSelect,
  icon,
  children,
}: {
  current: Mode;
  target: Mode;
  onSelect: (m: Mode) => void;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  const active = current === target;
  return (
    <button
      onClick={() => onSelect(target)}
      aria-pressed={active}
      className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs transition ${
        active
          ? "bg-text-primary text-bg-base"
          : "bg-bg-raised text-text-muted hover:text-text-primary"
      }`}
    >
      {icon}
      {children}
    </button>
  );
}

// ── PDA + program-id constants for the Anchor accounts context ──

function deriveConfigPda(programId: PublicKey): PublicKey {
  const [pda] = PublicKey.findProgramAddressSync([Buffer.from("config")], programId);
  return pda;
}

const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ATA_PROGRAM_ID = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
// System Program ID — 32 base58-encoded "1" characters (decodes to 32 zero
// bytes). Earlier copies of this string in this file had 41+ characters,
// which overflows the 32-byte buffer that PublicKey expects.
const SYSTEM_PROGRAM_ID = new PublicKey("11111111111111111111111111111111");
