# Architecture

This document describes the verified state of the codebase. It is the
self-documenting handoff for any new agent or contributor. Update it
when the architecture changes — additions only, no application behaviour
edits in this file.

## Top-level shape

```
crypto-exchange/
├── programs/exchange/        # Anchor program (Solana, vault-based)
├── backend/                   # Cargo workspace (3 binaries + 1 lib)
│   └── crates/
│       ├── common/            # shared types (Order, Trade, EngineEvent, ...)
│       ├── api/               # actix-web: stateless REST + WS + SIWS auth
│       ├── orderbook/         # actix-web admin + matching engine (stateful)
│       └── settler/           # consumes Fill events, calls Anchor
└── frontend/                  # Next.js 14 App Router (Vercel)
```

`CLAUDE.md` at the repo root is the per-session bootstrap doc (per-machine,
gitignored). `agents.md` is the human-facing handoff checklist for fresh
sessions.

## Data flow

```
              ┌──────────────────────────────────────────────────────────┐
   Wallet     │                       Frontend (Vercel)                   │
  (Phantom)   │  Next.js + Wallet Adapter + Anchor client                │
              │  pages: /, /trade/[symbol]                                │
              └─────────┬────────────────────────────────────────────────┘
                        │ HTTPS REST + WSS
                        ▼
              ┌──────────────────────────────────────────────────────────┐
              │  api server (actix-web, stateless, :8080)                │
              │  - JWT-gated REST for place/cancel/amend/balances         │
              │  - SIWS: nonce + verify -> HS256 JWT                       │
              │  - WS proxy of `events:outgoing` with subscription        │
              │    tracking on reconnect                                  │
              └─────────┬────────────────────────────────────────────────┘
                        │ XADD orders:incoming
                        ▼
              ┌────────────────────────────┐
              │  Redis Streams             │
              │  - orders:incoming         │
              │  - events:outgoing         │
              │  - settle:updates          │
              └─────────┬──────────────────┘
                        │ XREADGROUP / XREAD
                        ▼
              ┌──────────────────────────────────────────────────────────┐
              │  orderbook server (actix-web admin :8081)                │
              │  - in-memory MatchingEngine per symbol                   │
              │    BTreeMap<Price,VecDeque<OrderId>> + HashMap<OrderId,  │
              │    Order>; FIFO, price-time priority, SMP cancel-maker   │
              │  - consumer task: orders:incoming -> engine              │
              │  - candle aggregator (1m / 5m / 1h)                     │
              │  - market-maker bot keeps the book alive                 │
              │  - snapshot every 10s + replay on restart                │
              └─────────┬────────────────────────────────────────────────┘
                        │ XADD events:outgoing
                        ▼
              ┌──────────────────────────────────────────────────────────�
              │  settler worker (separate process)                       │
              │  - XREAD events:outgoing, filter EngineEvent::Fill       │
              │  - build + submit settle_fill Anchor ix                   │
              │  - retries with exponential backoff                       │
              │  - XADD settle:updates with status + signature           │
              └─────────┬────────────────────────────────────────────────┘
                        │ RPC to devnet
                        ▼
              ┌──────────────────────────────────────────────────────────┐
              │  Anchor program: vault-based custody + settle_fill       │
              │  PDAs: config, vault_authority,                          │
              │  user_vault:<pubkey>:<mint>, settlement_record:<buy>:<sell>│
              │  instructions: initialize, set_paused,                    │
              │  deposit_sol/usdc, withdraw_sol/usdc, settle_fill        │
              └──────────────────────────────────────────────────────────┘
```

## Key files

### Matching engine (the heart)
- `backend/crates/orderbook/src/engine.rs` — pure functions
  (`match_entry`, `cancel`, `amend`, `trigger_stops`). Returns
  `Vec<EngineEvent>`; no I/O. Caller-side code handles persistence and
  broadcast.
- `backend/crates/orderbook/src/orderbook.rs` — `BookSide` (price-keyed
  FIFO queues) and `OrderBook` wrapper with `top_of_book`,
  `depth_snapshot`, `insert`, `cancel`.
- `backend/crates/orderbook/src/candle.rs` — multi-interval OHLCV
  aggregator; tracks 1m/5m/1h buckets per symbol.

### Anchor program
- `programs/exchange/programs/exchange/src/lib.rs` — six instructions
  (initialize, set_paused, deposit_sol, deposit_usdc, withdraw_sol,
  withdraw_usdc, settle_fill).
- `programs/exchange/programs/exchange/src/state.rs` — `Config`,
  `UserBalance`, `SettlementRecord` account layouts with `SIZE` consts.
- `programs/exchange/target/idl/exchange.json` — generated IDL,
  consumed by the frontend via `frontend/src/lib/anchorClient.ts`.

### Frontend
- `frontend/src/providers/WalletProvider.tsx` — ConnectionProvider +
  WalletProvider (Phantom/Solflare/Backpack) + WalletModalProvider.
- `frontend/src/lib/anchorClient.ts` — PDA derivations
  (`vault_authority`, `user_vault`) + `useExchangeProgram` hook.
- `frontend/src/components/BalanceDisplay.tsx` — reads vault token
  account data directly via RPC (u64 amount at byte 64); refetches every
  10s.
- `frontend/src/components/DepositWithdraw.tsx` — calls Anchor deposit /
  withdraw ixs via the Program; auto-creates the user's ATA before
  deposit.
- `frontend/src/components/Orderbook.tsx`,
  `TradeTape.tsx`, `OrderForm.tsx`, `MarketSwitcher.tsx`,
  `CandleChart.tsx` — trading surface at `/trade/[symbol]`.
- `frontend/src/hooks/useWebSocket.ts` — reconnecting WS with
  subscription tracking across reconnects; sends `subscribe`/`ping`
  ops, queues subscriptions to flush on reconnect.
- `frontend/src/hooks/useFeeds.ts` — REST snapshot hydrate + WS delta
  fan-out into Zustand stores (`bookStore`, `tradeStore`,
  `candleStore`, `uiStore`).

## Build order (priority order)

1. Scaffold monorepo
2. Anchor program (deploy to devnet before settler becomes live)
3. `common` crate types
4. **Orderbook server** — the priority 1 deliverable
5. API server (REST + WS + SIWS auth)
6. Settler worker
7. Frontend wallet + deposit/withdraw
8. Frontend trading UI
9. Candle chart
10. Polish + deploy

## Conventions (verified from the code)

- All Rust binaries share `common` types — never duplicate wire types
  between crates.
- Engine is pure (`Vec<EngineEvent>` in, `Vec<EngineEvent>` out). I/O
  lives one layer up.
- Decimals (`rust_decimal::Decimal`) serialize as **strings** on the
  wire. JS parsers round-trip without f64 loss.
- CORS via `actix-cors`; origins from `ALLOWED_ORIGINS`. WS handler
  validates the `Origin` header explicitly.
- JWT: HS256, 24h TTL, `sub` claim = wallet pubkey. Mutating REST routes
  require Bearer JWT; unauthenticated routes (e.g. `/api/orderbook/:sym`)
  work for read-only traffic.
- `CLAUDE.md` is per-machine bootstrap only — **gitignored**.
- Commit messages are simple, no `Co-Authored-By:` footer.

## Build / run

```bash
# Redis (required)
docker compose up -d redis

# Backend (three binaries)
cd backend
cargo run -p orderbook    # :8081 admin + engine + bot
cargo run -p settler      # consumes Fill events
cargo run -p api          # :8080 REST + WS

# Frontend
cd frontend
npm install
npm run dev              # :3000
```

Or root `make dev` runs Redis + backend (parallel) + frontend. `make
backend` runs the three binaries in parallel.

## Testing

- `cargo test --workspace` — 9 tests pass (3 in `common`, 6 in
  `orderbook`).
- `npx tsc --noEmit` (in `frontend/`) — clean.
- `anchor test` in `programs/exchange/` — TypeScript happy path +
  idempotency + self-trade; requires local `anchor test` infra
  (validator + ts-mocha).
