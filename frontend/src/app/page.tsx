"use client";

import { WalletButton } from "@/components/WalletButton";
import { BalanceDisplay } from "@/components/BalanceDisplay";
import { DepositWithdraw } from "@/components/DepositWithdraw";

export default function Page() {
  return (
    <main className="mx-auto max-w-3xl px-6 py-10 md:py-16">
      <header className="mb-10 flex items-center justify-between">
        <div>
          <p className="text-xs uppercase tracking-[0.18em] text-text-dim">CEX Demo</p>
          <h1 className="mt-1 text-2xl font-semibold tracking-tight text-text-primary">
            SOL/USDC wallet
          </h1>
        </div>
        <WalletButton />
      </header>

      <div className="grid gap-6 md:grid-cols-2">
        <BalanceDisplay />
        <DepositWithdraw />
      </div>

      <footer className="mt-12 text-xs text-text-dim">
        Devnet. Custody is a vault PDA on the Anchor exchange program; deposit and
        withdraw instructions settle on Solana.
      </footer>
    </main>
  );
}
