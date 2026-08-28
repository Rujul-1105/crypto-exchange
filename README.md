# CEX Demo

A **Centralized Exchange** demo showcasing:

1. **Distributed orderbook architecture** — stateless actix-web API tier + stateful orderbook server + settler worker, bridged by **Redis Streams**.
2. **Matching engine** — limit / market (IOC) / stop / stop-limit orders, FIFO price-time priority, partial fills, cancel/amend, self-match prevention (cancel-maker).
3. **Solana settlement** — vault-based Anchor program (`deposit` / `withdraw` / `settle_fill`), wSOL + USDC on devnet.
4. **Wallet-connected frontend** — Next.js + Solana Wallet Adapter + TradingView lightweight-charts.

Read **[CLAUDE.md](./CLAUDE.md)** first — it's the bootstrap doc every fresh agent reads to pick up the work, with phase status and the build order.

Full design plan: `/home/rujul/.claude/plans/we-will-be-building-compressed-noodle.md`.

## Repo Layout

```
crypto-exchange/
├── CLAUDE.md              # session bootstrap (READ FIRST)
├── Makefile               # convenience commands
├── docker-compose.yml     # Redis
├── .env.example           # copy to .env and fill in
├── programs/exchange/     # Anchor program (Solana)
│   └── programs/exchange/src/{lib.rs, state.rs, errors.rs}
├── backend/               # Cargo workspace
│   └── crates/
│       ├── common/        # shared types
│       ├── api/           # actix-web REST + WS, stateless
│       ├── orderbook/     # matching engine + Redis consumer, stateful
│       └── settler/       # consumes fills, calls Anchor settle_fill
└── frontend/              # Next.js 14 App Router
    └── src/{app, components, hooks, stores, lib, providers}
```

## Quick Start (local)

```bash
# 1. Bring up Redis
make redis-up

# 2. Copy envs
cp .env.example .env
cp backend/.env.example backend/.env
cp frontend/.env.example frontend/.env.local

# 3. (Phase 2+) build the Anchor program + deploy to devnet
make anchor-build
make anchor-deploy-devnet   # writes the program id; paste into backend/.env + frontend/.env.local

# 4. Build + run the backend (3 binaries)
make build-backend
make backend               # api :8080, orderbook :8081, settler

# 5. Frontend
make frontend-install
make frontend              # http://localhost:3000
```

For end-to-end dev (Redis + backend + frontend):

```bash
make dev
```

## Architecture (one-liner)

```
Wallet → Next.js → API server (stateless)
                     └─► Redis Streams (orders:incoming)
                              └─► Orderbook server (in-memory matching engine)
                                    ├─► Redis Streams (events:outgoing) ──► API WS broadcast
                                    └─► Redis (settle:updates) ──► Settler worker
                                                                        └─► Anchor settle_fill on devnet
```

## Phase Status

See **[CLAUDE.md](./CLAUDE.md#phase-status-update-at-end-of-every-phase)**. Currently: **Phase 1 — Scaffold complete.**

## License

MIT
