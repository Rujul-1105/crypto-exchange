# Custodial-Simulation Crypto Exchange

A Rust implementation of a centralized exchange (CEX) **internally simulated**.
This is a portfolio project, not a product.

## Non-negotiable non-goals

These are out of scope **permanently**, not "not yet":

- **No real wallets, no real keys, no on-chain custody.** All balances are
  internal ledger entries. There are no signing operations, no key
  management, no chain integrations.
- **No fiat rails.** No bank connectors, no payment processors, no
  stablecoin off-ramps, no FX.
- **No real KYC.** There is an `admin` role, but no identity verification,
  sanctions screening, or any of the regulatory machinery that real
  exchanges are required to have.
- **No cross-exchange liquidity.** No market-data ingestion from other
  venues, no arbitrage paths, no external market making.

The point of this project is to demonstrate, in priority order:

1. A correct, fast, well-tested matching engine.
2. A correct double-entry ledger with atomic settlement.
3. Reasonable concurrency design.
4. Standard secure API / auth practices.

If you find yourself adding any of the non-goals above, stop — they are
explicitly excluded so that reviewer attention stays on the four
demonstration goals.

## Layout

```
frontend/                # Next.js SPA consuming the REST API (Phase 7)
crates/
├── common/           # shared types: OrderId, Price, Qty, Symbol, Side, ...
├── matching-engine/  # pure lib, zero async/db deps — the centerpiece
├── actors/           # per-symbol tokio actors wrapping the matching engine (Phase 2)
├── ledger/           # double-entry accounts (Postgres arrives Phase 3)
└── api/              # HTTP surface (Axum arrives Phase 4/5)
```

See `CLAUDE.md` for the full phase-by-phase plan and exit criteria.

## Build & test

```sh
cargo build
cargo test
cargo bench -p matching-engine    # Phase 1+

# Frontend (Phase 7, docs only — code not yet committed)
# cd frontend && npm install && npm run dev
```

## Phase status

- **Phase 0 — Scope Lock & Repo Setup** ✅
- **Phase 1 — Order Book Core** ✅
- **Phase 2 — Concurrency Layer** ✅
- Phase 3 — Ledger (Double-Entry, Atomic Settlement)
- Phase 4 — Auth & RBAC
- Phase 5 — REST API & Persistence Integration
- Phase 6 — Hardening & Writeup
- Phase 7 — Frontend (Next.js SPA) — planned, docs only