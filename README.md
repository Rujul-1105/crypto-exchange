# CEX Demo

A **Centralized Exchange** demo showcasing:

1. **Distributed orderbook architecture** — stateless actix-web API tier + stateful orderbook server + settler worker, bridged exclusively by **Redis Streams**.
2. **Matching engine** — limit / market (IOC) / stop / stop-limit orders, FIFO price-time priority, partial fills, cancel/amend, self-match prevention (cancel-maker).
3. **Solana settlement** — vault-based Anchor program (`deposit` / `withdraw` / `settle_fill`), wSOL + USDC on devnet.
4. **Wallet-connected frontend** — Next.js + Solana Wallet Adapter + TradingView lightweight-charts.

## What works end-to-end

Verified on Solana devnet with a market-maker bot populating `SOL/USDC` at mid ≈ $150:

1. **SIWS auth** — `POST /api/auth/nonce` → sign in Phantom → `POST /api/auth/verify` → HS256 JWT in the Zustand session store.
2. **Deposit SOL** — frontend auto-wraps native SOL → wSOL into the user's wSOL ATA, then `depositSol` instruction transfers wSOL into the shared vault and bumps the `user_balance` PDA.
3. **Place a market buy** — `OrderForm` sends to `/api/orders`, `XADD orders:incoming`, orderbook engine matches against the bot's resting ask, emits `Fill`/`Trade`/`BookDelta` events on `events:outgoing`.
4. **On-chain settlement** — settler worker reads `Fill`, builds the `settleFill` Anchor instruction with `SettlementRecord` PDA idempotency, submits to Solana.
5. **Balance updates** — `BalanceDisplay` reads `user_balance` via `program.account.userBalance.fetch`; settler `SettleUpdate` event flips `Trade.settle_status`; UI refetches every 5s.

## Stack

- **Backend** — Rust 2021, actix-web 4, redis 0.27, tokio, ed25519-dalek, jsonwebtoken, rust_decimal, Anchor 0.31 Rust client
- **Frontend** — Next.js 14 (App Router), TypeScript, TanStack Query, Zustand, @coral-xyz/anchor 0.31, @solana/web3.js 1.95, lightweight-charts
- **Solana** — Anchor 0.31 program deployed to devnet (program id `DNhvifJ6mcgVRRA4xaNKH82tjoHseN3GQKiLi8R7i6HY`)

## Repo Layout

```
crypto-exchange/
├── README.md                # this file
├── ARCHITECTURE.md          # verified data flow + key files
├── agents.md                # handoff notes for fresh sessions
├── Makefile                 # redis-up, backend, frontend, dev
├── docker-compose.yml       # Redis only
├── .env.example
├── programs/exchange/       # Anchor program (Solana)
│   └── programs/exchange/src/
│       ├── lib.rs           # instruction handlers
│       ├── instructions.rs  # 7 #[derive(Accounts)] structs
│       ├── state.rs         # Config, UserBalance, SettlementRecord
│       └── errors.rs        # ExchangeError enum
├── backend/                 # Cargo workspace
│   └── crates/
│       ├── common/          # shared types (Order, Trade, EngineEvent, ...)
│       ├── api/             # actix-web REST + WS + SIWS auth, stateless
│       ├── orderbook/       # matching engine + Redis consumer, stateful
│       └── settler/         # consumes fills, calls Anchor settle_fill
└── frontend/                # Next.js 14 App Router
    └── src/{app, components, hooks, stores, lib, providers}
```

## Quick Start (local dev)

```bash
# 1. Bring up Redis
make redis-up

# 2. Copy envs
cp .env.example .env
cp frontend/.env.example frontend/.env.local
# backend/.env ships pre-filled with the deployed devnet program id

# 3. Run the backend (3 services in parallel)
make backend                 # api :8080, orderbook :8081, settler

# 4. Run the frontend
make frontend-install
make frontend                # http://localhost:3000
```

Or all-at-once:

```bash
make dev
```

Phantom must be on **devnet** and have ≥0.5 SOL for the demo flow.

## Architecture

```
Wallet (Phantom) -> Next.js
                     └─ REST + WS -> API server (stateless)
                                       └─ XADD orders:incoming -> Redis Streams
                                                                    └─ XREAD -> Orderbook server (in-memory engine)
                                                                                          ├─ XADD events:outgoing -> API WS broadcast
                                                                                          ├─ XADD events:outgoing -> Settler worker
                                                                                          │                              └─ Anchor settle_fill -> Solana devnet
                                                                                          └─ JSON snapshot every 10s + stream-replay on restart
```

Deep dive in **[ARCHITECTURE.md](./ARCHITECTURE.md)** (verified state, data flow, wire protocol, conventions).

## Testing

- **Workspace unit tests** — `cargo test --workspace` (9 passing: 3 in `common`, 6 in `orderbook::engine::tests` covering FIFO, partial-fill queue ordering, SMP, stop trigger, market IOC, FOK pre-check).
- **Anchor integration tests** — `cd programs/exchange && anchor test` (TypeScript happy path + idempotency + self-trade; requires local validator).

## Deployment

Backend runs on a personal VM behind a Caddy reverse proxy (TLS + WSS). Frontend is Vercel. Step-by-step runbook in `frontend/.local/deploy.md` (untracked).

## License

MIT