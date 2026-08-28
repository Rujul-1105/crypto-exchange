import Link from "next/link";

export default function Page() {
  return (
    <main className="mx-auto max-w-5xl p-8">
      <header className="mb-12">
        <h1 className="text-3xl font-semibold tracking-tight">CEX Demo</h1>
        <p className="mt-2 text-text-muted">
          Distributed orderbook + matching engine + Solana settlement.
        </p>
      </header>

      <section className="grid grid-cols-1 gap-6 md:grid-cols-3">
        <Card title="Orderbook" href="/trade/SOL-USDC">
          Live bid/ask ladder, snapshot-then-deltas over WebSocket.
        </Card>
        <Card title="Trades & Candles" href="/trade/SOL-USDC">
          Real-time tape and candlestick chart.
        </Card>
        <Card title="Wallet" href="/wallet">
          Connect Phantom/Solflare, deposit devnet SOL/USDC, trade.
        </Card>
      </section>

      <section className="mt-12 rounded border border-line bg-bg-surface p-6 text-sm text-text-muted">
        <strong className="text-text-primary">Phase 1 scaffold.</strong> The trading UI lands in
        Phases 7-9. For now, you can hit the backend directly with{" "}
        <code className="rounded bg-bg-raised px-1 py-0.5 font-mono">curl</code> or{" "}
        <code className="rounded bg-bg-raised px-1 py-0.5 font-mono">wscat</code>. See{" "}
        <code className="rounded bg-bg-raised px-1 py-0.5 font-mono">CLAUDE.md</code> at the repo
        root for the full plan.
      </section>
    </main>
  );
}

function Card({ title, href, children }: { title: string; href: string; children: React.ReactNode }) {
  return (
    <Link
      href={href}
      className="rounded border border-line bg-bg-surface p-6 transition hover:border-line-strong hover:bg-bg-raised"
    >
      <h2 className="text-lg font-medium">{title}</h2>
      <p className="mt-2 text-sm text-text-muted">{children}</p>
    </Link>
  );
}
