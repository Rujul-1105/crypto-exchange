"use client";

import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { PublicKey } from "@solana/web3.js";
import { useQuery } from "@tanstack/react-query";
import { Coin } from "@phosphor-icons/react";

import { MINTS, deriveUserVaultAta, deriveVaultAuthorityPda, useExchangeProgram } from "@/lib/anchorClient";
import { truncateAddress, formatAmount } from "@/lib/format";

const LAMPORTS_PER_SOL = 1_000_000_000;

type TokenBalance = {
  symbol: "SOL" | "USDC";
  mint: PublicKey;
  vaultAta: PublicKey;
  decimals: number;
  free: number;
  locked: number;
};

function useTokenBalances() {
  const { connection } = useConnection();
  const { publicKey } = useWallet();
  const program = useExchangeProgram();

  return useQuery({
    queryKey: ["balances", publicKey?.toBase58() ?? null],
    enabled: !!publicKey && !!program,
    refetchInterval: 10_000,
    queryFn: async (): Promise<TokenBalance[]> => {
      if (!publicKey || !program) throw new Error("not ready");
      const programId = program.programId;
      const vaultAuthority = deriveVaultAuthorityPda(programId);

      const solVault = deriveUserVaultAta(vaultAuthority, MINTS.SOL);
      const usdcVault = deriveUserVaultAta(vaultAuthority, MINTS.USDC);

      const [solInfo, usdcInfo] = await Promise.all([
        connection.getAccountInfo(solVault),
        connection.getAccountInfo(usdcVault),
      ]);

      const parseTokenAmount = (info: typeof solInfo): number => {
        if (!info) return 0;
        // SPL token account data layout: amount is u64 at offset 64.
        const view = new DataView(info.data.buffer, info.data.byteOffset, info.data.byteLength);
        const low = view.getUint32(64, true);
        const high = view.getUint32(68, true);
        return low + high * 2 ** 32;
      };

      const solAmount = parseTokenAmount(solInfo) / LAMPORTS_PER_SOL;
      const usdcAmount = parseTokenAmount(usdcInfo) / 1_000_000;

      return [
        { symbol: "SOL" as const, mint: MINTS.SOL, vaultAta: solVault, decimals: 9, free: solAmount, locked: 0 },
        { symbol: "USDC" as const, mint: MINTS.USDC, vaultAta: usdcVault, decimals: 6, free: usdcAmount, locked: 0 },
      ];
    },
  });
}

export function BalanceDisplay() {
  const { publicKey } = useWallet();
  const { data, isLoading, error } = useTokenBalances();

  if (!publicKey) {
    return (
      <div className="rounded-xl border border-line bg-bg-surface p-6">
        <p className="text-sm text-text-muted">Connect a wallet to view balances.</p>
      </div>
    );
  }

  if (isLoading) {
    return <BalanceSkeleton />;
  }

  if (error) {
    return (
      <div className="rounded-xl border border-line bg-bg-surface p-6">
        <p className="text-sm text-rose-400">Balance unavailable. Retrying.</p>
      </div>
    );
  }

  const rows = data ?? [];

  return (
    <section
      aria-label="Wallet balances"
      className="rounded-xl border border-line bg-bg-surface"
    >
      <header className="flex items-center justify-between border-b border-line px-5 py-4">
        <h2 className="text-sm font-medium text-text-primary">Balances</h2>
        <p className="font-mono text-xs text-text-dim">{truncateAddress(publicKey.toBase58())}</p>
      </header>

      <ul className="divide-y divide-line">
        {rows.map((row) => (
          <li key={row.symbol} className="flex items-center justify-between px-5 py-4">
            <div className="flex items-center gap-3">
              <span className="grid h-9 w-9 place-items-center rounded-full bg-bg-raised">
                <Coin size={18} weight="regular" className="text-text-primary" />
              </span>
              <div>
                <p className="text-sm font-medium text-text-primary">{row.symbol}</p>
                <p className="font-mono text-xs text-text-dim">
                  vault {truncateAddress(row.vaultAta.toBase58())}
                </p>
              </div>
            </div>
            <div className="text-right">
              <p className="font-mono text-base text-text-primary">
                {formatAmount(row.free, row.symbol === "SOL" ? 4 : 2)}
              </p>
              <p className="text-xs text-text-dim">on-chain</p>
            </div>
          </li>
        ))}
      </ul>

      {rows.length === 0 && (
        <div className="px-5 py-6 text-center text-sm text-text-muted">
          No vault accounts yet. Deposit SOL or USDC to open one.
        </div>
      )}
    </section>
  );
}

function BalanceSkeleton() {
  return (
    <div
      aria-busy="true"
      aria-label="Loading balances"
      className="rounded-xl border border-line bg-bg-surface"
    >
      <header className="border-b border-line px-5 py-4">
        <div className="h-4 w-24 animate-pulse rounded bg-bg-raised" />
      </header>
      <ul className="divide-y divide-line">
        {[0, 1].map((i) => (
          <li key={i} className="flex items-center justify-between px-5 py-4">
            <div className="flex items-center gap-3">
              <div className="h-9 w-9 animate-pulse rounded-full bg-bg-raised" />
              <div className="space-y-2">
                <div className="h-3 w-16 animate-pulse rounded bg-bg-raised" />
                <div className="h-3 w-32 animate-pulse rounded bg-bg-raised" />
              </div>
            </div>
            <div className="h-5 w-24 animate-pulse rounded bg-bg-raised" />
          </li>
        ))}
      </ul>
    </div>
  );
}
