# Agents — handoff notes for a fresh Claude Code session

> Read `CLAUDE.md` (per-machine bootstrap) and `ARCHITECTURE.md`
> (verified repo state) before doing anything. This file is the
> short-list of things a fresh session needs to know: where the rough
> edges are, what's stubbed, what not to break.

## Files created / modified by the doc pass

- `ARCHITECTURE.md` (new) — verified repo state, data flow, key files,
  build/run, conventions. Repo-committed.
- `agents.md` (new, this file) — handoff notes. Repo-committed.

## Architectural discoveries

1. **The orderbook admin does NOT expose every endpoint the API
   proxies to it.** The orderbook admin (`backend/crates/orderbook/src/main.rs`
   lines 96-105) registers only:
   - `GET /health`
   - `GET /api/symbols`
   - `GET /api/orderbook/:symbol`
   - `GET /api/trades/:symbol`

   But the API (`backend/crates/api/src/main.rs` lines 67-77) registers
   a `/candles/:symbol/:interval` route and a `/balances` route that
   proxy to the orderbook admin. Both will currently return **404 / 502**
   because no upstream route exists. See "Known technical debt"
   below.

2. **The orderbook consumer dispatches nothing.** `redis_consumer.rs::dispatch`
   takes `Arc<()>` (a placeholder) and logs the command without
   running it through the engine. The consumer reads orders off
   `orders:incoming` and discards them after logging.

   This means: **as of now, placing an order via the API / UI does not
   result in a real order in the matching engine**. The market-maker
   bot populates the book independently (it calls the engine directly
   via `Arc<Mutex<MatchingEngine>>`), so the demo still *looks* alive
   in the browser, but no human-placed order ever matches or rests on
   the book.

3. **Settler submit is stubbed.** `settler/src/anchor_client.rs::submit`
   (line 74) returns a deterministic `fake-sig-<id>` string instead of
   building + sending a real transaction. The full retry loop, status
   publishing, and event filtering work — but on-chain settlement
   does not happen. The comment block in `submit` sketches the real
   path.

4. **WS subscription filtering is incomplete.** `api/src/ws.rs`
   tracks `self.subscriptions` (HashSet<String>) on inbound subscribe
   messages, but the broadcast path (`PushText` handler) sends every
   `EventEnvelope` from `events:outgoing` regardless of subscription
   or `user_pubkey`. So clients receive all events for all symbols,
   not just what they subscribed to.

5. **Order `list_orders`/`get_order`/`balances` REST endpoints** proxy
   to orderbook admin endpoints that don't exist (see #1). The place
   + cancel + amend + auth paths work end-to-end.

## Known technical debt

- **Orderbook dispatch stub** (`redis_consumer.rs`). Highest-priority
  fix: replace `dispatch(Arc::clone(&registry_inners(&registry).await), &cmd, &bus)`
  with proper symbol routing, e.g.
  ```rust
  let engine = registry.get_or_create(symbol_for(&cmd)).await;
  let mut eng = engine.lock().await;
  let events = eng.match_entry(to_order(&cmd), now_ms());
  for e in events { bus.publish_event(&symbol, &e).await.ok(); }
  ```
  After fixing this, the order flow becomes end-to-end live.

- **Settler live submit** (`anchor_client.rs::submit`). The skeleton
  is in the file (see comment lines 76-83). Implementation requires:
  - Constructing the `settle_fill` instruction with the right
    accounts (config PDA, settlement_record PDA seeds
    `[b"settlement", buy_id.to_le_bytes(), sell_id.to_le_bytes()]`,
    buyer + seller `UserBalance` PDAs, system_program).
  - Fetching the latest blockhash and submitting via
    `solana_client::RpcClient::send_and_confirm_transaction`.
  - Returning the actual signature string.

- **Missing admin endpoints** (`orderbook/src/main.rs`). Add:
  - `GET /api/candles/:symbol/:interval` — returns the in-memory
    `Candle` array from `CandleAggregator::snapshot`.
  - `GET /api/orders/:id` — looks up the order in the engine.
  - `GET /api/orders` (list) — filtered by user/symbol/status.
  - `GET /api/balances` — returns the user's `user_balance` PDA data
    (or fetches from on-chain; current frontend reads on-chain
    directly).

- **WS subscription filter** (`api/src/ws.rs`). The `PushText`
  handler should filter by `self.subscriptions` and (for the
  `orders:<sym>` channel) by `self.user_pubkey`.

- **Anchor program has account contexts but no `#[instruction(...)]`
  on `SettleFill`** — wait, it does. Just confirming: the
  `SettleFill` struct has
  `#[instruction(buy_order_id: u64, sell_order_id: u64, _price: u64, _quantity: u64)]`
  and `settler` is `#[account(mut)]` (pays for the
  `settlement_record` init). This is correct.

- **`api/src/ws.rs` has unused fields**: `user_pubkey` is read on
  the first message only (to validate JWT), not used in broadcast
  filter; `subscriptions` is stored but never gates the broadcast.
  Both are pre-stubs for the proper subscription filter.

- **`anchor_client.rs::settler` keypair** is held by
  `SettlerClient` but never used (the live submit path isn't
  implemented). Will be used when the live submit lands.

- **`use_feeds` re-subscribes on every render** because
  `subscribe`/`unsubscribe` are `useCallback`s that don't have stable
  deps. Functionally correct (server treats re-subscribe as
  idempotent) but wasteful. Wrap in `useCallback` with stable deps or
  use a ref pattern.

- **`backend/Cargo.toml`** has an unused `[workspace.dev-dependencies]`
  block (warning). Remove or move into a member crate's
  `[dev-dependencies]`.

- **`programs/exchange/programs/exchange/src/lib.rs`** has two
  account-context structs (`DepositSol`, `WithdrawSol`,
  `DepositUsdc`, `WithdrawUsdc`) that are largely identical; consider
  factoring a macro if more mints get added.

## Current unfinished work

The user explicitly accepted the demo at the end of Phase 10. Nothing
is actively in flight. The highest-value next moves are:

1. **Wire the orderbook consumer** so human-placed orders reach the
   engine (fix #1 / #2 in "Architectural discoveries").
2. **Implement live settlement** in the settler (`submit`).
3. **Add the missing admin endpoints** so the API's proxy chain
   resolves cleanly.
4. **Filter WS by subscription** for sane network traffic.

## Things to NOT break

- The matching engine invariants in
  `backend/crates/orderbook/src/engine.rs`. Pure functions returning
  `Vec<EngineEvent>`. The level-removal-after-pop ordering (pop → drop
  empty level BEFORE the SMP check) is a real invariant — see git
  history comment.
- `rust_decimal` string serialization on the wire. Any change to
  `Price`/`Quantity` JSON shape will break the frontend.
- The `SettlerFill` Anchor instruction's idempotency PDA seeds:
  `[b"settlement", buy_id.to_le_bytes(), sell_id.to_le_bytes()]`.
  Changing those breaks the on-chain dedup.
- `Cargo.lock` should be committed for reproducible builds
  (already committed in this repo).

## Verification commands

```bash
# Backend compiles?
cd backend && cargo check --workspace

# Backend tests pass?
cd backend && cargo test --workspace
# → 9 passed (3 common, 6 orderbook)

# Anchor program builds?
cd programs/exchange && anchor build
# → produces target/deploy/exchange.so (~377KB)

# Frontend typechecks?
cd frontend && npx tsc --noEmit

# Whole repo (no app behaviour changes here)
git status --short
```
