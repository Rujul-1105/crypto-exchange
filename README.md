# CEX Demo

A **Centralized Exchange** demo showcasing:

1. **Distributed orderbook architecture** - stateless actix-web API tier + stateful orderbook server + settler worker, bridged by **Redis Streams**.
2. **Matching engine** - limit / market (IOC) / stop / stop-limit orders, FIFO price-time priority, partial fills, cancel/amend, self-match prevention (cancel-maker).
3. **Solana settlement** - vault-based Anchor program (`deposit` / `withdraw` / `settle_fill`), wSOL + USDC on devnet.
4. **Wallet-connected frontend** - Next.js + Solana Wallet Adapter + TradingView lightweight-charts.

Read **[CLAUDE.md](./CLAUDE.md)** first - it's the bootstrap doc every fresh agent reads to pick up the work, with phase status and the build order.

Full design plan: `/home/rujul/.claude/plans/we-will-be-building-compressed-noodle.md`.

## Repo Layout

```
crypto-exchange/
├── CLAUDE.md              # session bootstrap (READ FIRST)
├── Makefile               # convenience commands
├── docker-compose.yml     # Redis
├── .env.example
├── programs/exchange/     # Anchor program (Solana)
│   └── programs/exchange/src/{lib.rs, state.rs, errors.rs}
├── backend/               # Cargo workspace
│   └── crates/
│       ├── common/        # shared types
│       ├── api/           # actix-web REST + WS, stateless
│       ├── orderbook/     # matching engine + Redis consumer, stateful
│       └── settler/       # consumes fills, calls Anchor settle_fill
├── frontend/              # Next.js 14 App Router
│   └── src/{app, components, hooks, stores, lib, providers}
```

## Quick Start (local dev)

```bash
# 1. Bring up Redis
make redis-up

# 2. Copy envs
cp .env.example .env
cp backend/.env.example backend/.env
cp frontend/.env.example frontend/.env.local

# 3. Deploy the Anchor program to devnet
make anchor-build
make anchor-deploy-devnet       # paste the program id into backend/.env + frontend/.env.local

# 4. Run the backend
make backend                   # api :8080, orderbook :8081, settler

# 5. Run the frontend
make frontend-install
make frontend                  # http://localhost:3000
```

Or all-at-once:

```bash
make dev
```

## Architecture (one-liner)

```
Wallet (Phantom) -> Next.js (Vercel)
                     └─ REST + WS -> API server (stateless)
                                       └─ XADD orders:incoming -> Redis Streams
                                                                    └─ XREADGROUP -> Orderbook server (in-memory)
                                                                                          ├─ XADD events:outgoing -> API WS broadcast
                                                                                          ├─ XADD events:outgoing -> Settler worker
                                                                                          │                              └─ Anchor settle_fill -> Solana devnet
                                                                                          └─ snapshot every 10s + replay on restart
```

## Deployment

Backend services (`api`, `orderbook`, `settler`) run on the user's personal VM as three systemd units. The frontend is built and served by Vercel.

## Testing

- **Workspace tests**: `cargo test --workspace` (9 passing - 3 in `common`, 6 in `orderbook`).
- **Anchor test**: `cd programs/exchange && anchor test` (TypeScript happy path + idempotency + self-trade; needs `anchor test` infra running locally).

## Phase Status

See **[CLAUDE.md](./CLAUDE.md#phase-status-update-at-end-of-every-phase)**. All 11 phases (0-10) are complete.

## License

MIT
