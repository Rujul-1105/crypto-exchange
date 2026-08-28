# CEX Demo — Session Bootstrap

Repo: `/home/rujul/projects/a/crypto-exchange/` (empty, branch `main`, remote `git@github.com:Rujul-1105/crypto-exchange.git`).
Goal: demonstrate orderbook architecture → matching engine → Solana wallet frontend.

> **You are a fresh agent.** Read this file top-to-bottom before doing anything. It defines the architecture, the build order (= priority order), and the conventions. The full design plan lives at `/home/rujul/.claude/plans/we-will-be-building-compressed-noodle.md` — consult it for any detail not covered here.

## Phase Status (update at end of every phase)

| Phase | Status | Notes |
|---|---|---|
| 0 — CLAUDE.md | ✅ Done (2026-08-28) | This file. |
| 1 — Scaffold | ⏳ Pending | git init, Cargo workspace, Anchor init, Next.js init, docker-compose, Makefile. |
| 2 — Anchor program | ⏳ Pending | Vault-based; deposit/withdraw/settle_fill; deploy devnet. |
| 3 — `common` crate | ⏳ Pending | Shared types for all Rust bins. |
| 4 — Orderbook server (PRIORITY 1) | ⏳ Pending | Matching engine + Redis consumer + 10s snapshot/replay. |
| 5 — API server | ⏳ Pending | actix-web REST + WS + SIWS/JWT. |
| 6 — Settler worker | ⏳ Pending | Consume fills → Anchor settle_fill. |
| 7 — Frontend wallet + deposit/withdraw | ⏳ Pending | Wallet Adapter + Anchor client. |
| 8 — Frontend trading UI | ⏳ Pending | Orderbook, tape, form, open orders. |
| 9 — Candle chart | ⏳ Pending | lightweight-charts + backend aggregator. |
| 10 — Polish + deploy | ⏳ Pending | systemd, Vercel, smoke tests. |

## Architecture (locked)

- **API server(s)** — actix-web, **stateless**, horizontally scalable. Handles REST + WebSocket, Sign-In With Solana (SIWS) → JWT auth, Redis Streams producer.
- **Orderbook server** — actix-web admin + in-memory matching engine + Redis Streams consumer. **One replica** holds the live book.
- **Settler worker** — consumes `Fill` events from Redis, submits `settle_fill` Anchor instructions to Solana devnet.
- **Redis Streams** — the only bridge between API tier and orderbook. `orders:incoming` (API→orderbook), `events:outgoing` (orderbook→API+settler).
- **Anchor program** — vault-based: users deposit SOL/USDC into per-user, per-mint vault PDAs owned by a `vault_authority` PDA; `settle_fill` ix atomically transfers tokens between vaults on a fill.
- **Orderbook durability** — snapshot every 10s to `backend/snapshots/snap-<ts>.bin` + replay unconsumed Redis messages on restart (no unbounded WAL).
- **Frontend** — Next.js 14 App Router + TS + Solana Wallet Adapter (`@solana/wallet-adapter-react`); TradingView `lightweight-charts`. Deployed on Vercel. Backend on user's VM.

## Repo Layout

```
crypto-exchange/
├── CLAUDE.md              # THIS FILE — update at end of every phase
├── Makefile · docker-compose.yml · .env.example
├── programs/exchange/     # Anchor program (Solana)
├── backend/               # Cargo workspace: common, api, orderbook, settler
└── frontend/              # Next.js, Vercel
```

## Build Order = Priority Order

**Phase 0** — Write this CLAUDE.md.
**Phase 1** — Scaffold monorepo (workspace Cargo.toml, Anchor.toml, Next.js init, docker-compose for Redis, Makefile, .env.example).
**Phase 2** — Anchor program: `initialize`, `deposit_sol`/`deposit_usdc`, `withdraw_*`, `settle_fill`. Tests with Mollusk/LiteSVM + one `anchor test`. Deploy to devnet.
**Phase 3** — `common` crate: shared types (`Order`, `Trade`, `Side`, `OrderType`, `EngineEvent`).
**Phase 4** — **Orderbook server** (PRIORITY 1): in-memory matching engine (limit, market IOC, partial fills, cancel, amend, stop/stop-limit, SMP cancel-maker), Redis consumer for `orders:incoming`, snapshot every 10s + replay-on-restart, market-maker bot.
**Phase 5** — API server: actix-web REST + WebSocket, SIWS auth → JWT, Redis producer for `orders:incoming`, Redis subscriber → WS broadcast from `events:outgoing`.
**Phase 6** — Settler worker: consume `Fill` events, build + send `settle_fill` Anchor ix, retries, back-channel `settle:updates` stream.
**Phase 7** — Frontend wallet connect + deposit/withdraw (Next.js + Wallet Adapter + Anchor client).
**Phase 8** — Frontend trading UI (orderbook, trade tape, order form, open orders).
**Phase 9** — Candle aggregator (engine side) + lightweight-charts component.
**Phase 10** — Polish + deploy (systemd units for VM, Vercel env vars, smoke-test checklist).

## Match Engine Cheatsheet

- Book: `BTreeMap<Price, VecDeque<OrderId>>` per side + `HashMap<OrderId, Order>` for O(1) cancel/amend + `BTreeMap<Price, VecDeque<OrderId>>` for stops.
- FIFO per level: `push_back` on arrival, `pop_front` on match.
- Match at **resting price** (price-time priority).
- SMP: cancel-maker when taker matches own resting order.
- Stop triggers: per-trade evaluation via `trigger_stops(last_trade_price)` after each fill.
- Engine is **pure**: takes `&mut OrderBook`, returns `Vec<EngineEvent>`. Golden-vector unit tests.
- Decimals via `rust_decimal::Decimal`. Serialize decimals as **strings** on the wire (JSON).

## Anchor PDAs (programs/exchange)

- `config` — exchange config + authority
- `vault_authority` — PDA owning all user vault token accounts (signs `settle_fill`)
- `user_vault:<pubkey>:<mint>` — per-user, per-mint SPL token account
- wSOL for SOL leg (mint `So11111111111111111111111111111111111111112`)
- devnet USDC (mint `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU`)

## Redis Streams Topology

- `orders:incoming` — API XADDs `place`/`cancel`/`amend`; orderbook consumer-group reads
- `events:outgoing` — orderbook XADDs (`Accepted`, `Fill`, `BookDelta`, `Trade`, `CandleUpdate`, `Cancelled`, `Amended`, `Rejected`); API tier + settler subscribe (settler filters for `Fill`)
- `settle:updates` — settler XADDs `{trade_id, status, sig}`; orderbook consumes to flip `settle_status` and rebroadcast

## WS Protocol (snapshot-then-deltas)

Channels: `book:<sym>`, `trades:<sym>`, `candles:<sym>:<interval>`, `orders:<sym>` (JWT-gated, user-scoped).
Decimals as strings. Book snapshots sent on subscribe; deltas `{side, price, qty}` with `qty:"0"` meaning remove level.

## REST API (actix-web)

`/health`, `/api/symbols`, `/api/orderbook/:sym?depth=`, `/api/trades/:sym`, `/api/candles/:sym/:interval`, `/api/auth/nonce`, `/api/auth/verify` (SIWS → JWT), `/api/orders` (POST/GET), `/api/orders/:id` (GET/DELETE/PUT), `/api/balances`.

## Verification (end-to-end)

After Phase 6: curl health, list symbols, see bot-populated book, place limit via curl → WS shows order in book; place crossing market → trade on tape + on-chain `settle_fill` visible on Solscan; kill orderbook → restart → snapshot+replay recovers state.
After Phase 9: full UI flow — connect Phantom, deposit devnet SOL, place/cancel orders, watch candles tick, withdraw.

## Conventions

- All Rust bins share `common` crate types — never duplicate.
- Engine never does I/O. Persistence + WS broadcast happen in callers consuming `Vec<EngineEvent>`.
- CORS via `actix-cors`, origins from `ALLOWED_ORIGINS`. WS handler also validates `Origin`.
- No auth = demo-user fallback for un-authenticated routes; mutating routes always require JWT.
- **Update CLAUDE.md at the end of every phase** with what shipped + what's next + any deviations from this plan.
