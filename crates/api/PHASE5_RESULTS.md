# Phase 5 — REST API & Persistence Integration: Results

**Status:** ✅ exit criteria met. Phase 6 (Hardening & Writeup) is the next step.

## Verification commands

```sh
cargo build -p api
cargo test  --workspace
cargo test  -p api --test e2e
cargo tree  -p matching-engine -e normal --depth 1   # Phase 0 invariant
cargo run   -p api --bin exchange-server               # binary entry point
```

## Test results

| Suite | Tests | Notes |
|---|---|---|
| `crates/api/src/lib.rs` inline auth | 3 | role, password |
| `crates/api/src/idempotency.rs` inline | 3 | lookup + per-user + ttl eviction |
| `crates/api/src/ratelimit.rs` inline | 3 | burst, refill, per-user |
| `crates/api/src/wal.rs` inline | 4 | fresh-file header, replay order, reopen-seq, corrupt-line error |
| `crates/api/tests/auth_boundary.rs` (Phase 4) | 21 | unchanged |
| `crates/api/tests/e2e.rs` | **5** | full HTTP submit → book → drop → replay via WAL |
| All other crates (Phase 1/2/3) | 23 + 3 + 21 + 5 + 20 | unchanged |

**Total workspace: ~110 tests passing.**

## Phase 5 exit criterion — verified

> "end-to-end test: submit orders via HTTP, confirm trade execution, confirm balances update correctly, restart the process and confirm book state recovers from the event log."

The headline test, `e2e_resting_orders_persist_and_recover`, exercises:

1. `POST /auth/register` for two users.
2. `POST /admin/balances` (admin JWT) credits both.
3. `POST /orders` (user JWT) places two limit buys that rest on the book.
4. `GET /book/BTC-USDC` shows both bids (highest first).
5. Drop the live state. Inspect the WAL file: ≥2 Submit events.
6. Build a new `OrderService` against the same WAL. Run `bootstrap_from_wal`.
7. `GET /book/BTC-USDC` against the recovered state shows the same two bids in the same order.

**Result: book state recovers deterministically from the WAL.**

## Architecture

### AppState (`crates/api/src/http.rs`)

```rust
pub struct AppState {
    pub users: Arc<Mutex<InMemoryUserStore>>,
    pub ledger: Arc<Mutex<InMemoryLedger>>,
    pub service: Arc<OrderService>,
    pub actors: ActorRegistry,
    pub wal: Arc<Mutex<Wal>>,
    pub idempotency: Arc<IdempotencyCache>,
    pub rate_limit: Arc<RateLimiter>,
    pub jwt_secret: Vec<u8>,
}
```

### Routes

| Method | Path | Auth | Purpose |
|---|---|---|---|
| `POST` | `/auth/register` | public | Create user (User or MarketMaker; Admin requires admin promotion) |
| `POST` | `/auth/login` | public | Issue JWT |
| `GET`  | `/health` | public | Liveness |
| `GET`  | `/book/:symbol` | public | Snapshot of the book |
| `POST` | `/orders` | User | Submit limit/market order with optional `idempotency_key` |
| `DELETE` | `/orders/:order_id` | User | Cancel |
| `GET`  | `/balances` | User | Caller's per-asset balances |
| `POST` | `/admin/balances` | Admin | Manual credit/debit |
| `POST` | `/admin/users` | Admin | Create user (incl. Admin role) |
| `POST` | `/admin/symbols` | Admin | Register a new symbol + pre-spawn actor |
| `GET`  | `/admin/symbols` | Admin | List symbols |

### `OrderService::submit_order` orchestration (the centerpiece)

```
rate-limit → build PlaceOrder → ledger.place (locks funds) →
actor.submit_limit (match) → for each fill, ledger.settle_trade →
wal.append → return trades + resting/cancelled remainder.
```

### `AuthContext` extractor

`Authorization: Bearer <jwt>` is parsed and verified on every protected request via a `FromRequestParts<AppState> for AuthContext` impl. Returns `ApiError::Auth(AuthError::InvalidToken)` (401), `ApiError::Auth(AuthError::Forbidden)` (403), etc. The `ApiError: IntoResponse` impl maps to axum `StatusCode` + a JSON `{ "error": "..." }` body.

### WAL

JSONL file. Each non-header line is a `WalEvent { seq, action: Submit|Cancel, order_id, user_id, symbol, side, kind, price, qty, timestamp }`. Append-only, `flush()` after each line for durability.

`bootstrap_from_wal` reconstructs:
- the actor registry (one actor per symbol seen in events),
- the matching-engine book (each event is re-applied to a fresh actor via `submit_limit` / `cancel`).

The matching engine is deterministic given identical input, so the replayed book matches live.

### Idempotency

`IdempotencyCache { inner: Mutex<HashMap<(user_id, key), Entry>> }`. On submit, if the request carries an `idempotency_key`, we look it up; on hit, replay the cached response (status 200 instead of 201). TTL 300s; eviction is lazy on lookup.

### Rate limiting

Per-user token bucket: `capacity = 50`, `refill_per_sec = 10`. Returns `ServiceError::RateLimited` (→ HTTP 429 in handlers) when the bucket is empty. In-memory; reset on restart (acceptable for the demo scope).

### Binary entry point

`crates/api/src/bin/exchange_server.rs`. `WAL_PATH` and `BIND` env vars; defaults `./exchange.wal` and `0.0.0.0:8080`. Run with:

```
WAL_PATH=/var/exchange.wal BIND=0.0.0.0:8080 cargo run --bin exchange-server
```

## Phase 0 / Phase 1 invariants preserved

`cargo tree -p matching-engine -e normal --depth 1`:
```
matching-engine v0.1.0
└── common v0.1.0
```

Zero async/db deps in the matching engine. Phase 5 added them only to the `api` crate (which is the right place — Phase 4 already had auth; Phase 5 layers HTTP and persistence on top).

## Known limitations (deliberately out of scope this phase)

- **WAL records Submit/Cancel only**, not ledger mutations (admin credits, withdrawals, etc.). Restoring those would mean replay re-recording balance states, but admin credits aren't in the WAL today. **Effect:** after a process restart, the ledger is empty; the matching-engine book is correct. To get full state recovery, widen the WAL to record `Deposit` / `Withdraw` events and have `apply_event` call `ledger.deposit/withdraw` on replay. Phase 6+ widening.
- **No `trade_id` on settlement yet.** The ledger exposes trade IDs (Phase 3) but the e2e flow doesn't surface them in the response. Adding `{ "trade_id": N }` to `FillJson` is a trivial follow-up.
- **No first-user bootstrap via CLI.** The exchange-server binary does *not* seed an initial admin. Currently, an admin must be created by an HTTP call to `POST /admin/users` (which requires an existing admin's JWT). For Phase 5 demo, `app_state()` in the e2e test seeds one. Phase 6 should add a CLI flag or env-driven bootstrap.
- **JSON side/kind** in `SubmitOrderRequest` (string literals `"buy"`, `"sell"`, `"limit"`, `"market"`). This keeps `common` dep-free (Phase 0 invariant). A future phase could move to enums + a serde feature flag.
- **`AuthContext` extractor requires `AppState`** as the `S` parameter. This is fine for production but the API tests must construct `AppState` themselves (rather than using a generic `Router<S>`). Documented in the extractor.
- **No request body size limit.** axum defaults to 2MB; not yet exposed.
- **No per-user rate-limit on auth endpoints.** Login brute-force protection is Phase 6 work.

## Files added / modified

```
crates/api/Cargo.toml                              # +axum 0.7, +tokio, +tower, +serde_json, +futures, +actors
crates/api/src/lib.rs                              # module decls + re-exports
crates/api/src/error.rs                            # ApiError + IntoResponse impl
crates/api/src/auth.rs                             # AuthError::UnknownOrder (re-added) + Role serde derives
crates/api/src/wal.rs                              # WalHeader, WalEvent, WalAction, append + replay
crates/api/src/idempotency.rs                      # IdempotencyCache
crates/api/src/ratelimit.rs                        # BucketConfig, RateLimiter
crates/api/src/service.rs                          # OrderService, ActorRegistry, SubmitResult
crates/api/src/http.rs                             # axum router + AuthContext extractor + handlers
crates/api/src/bin/exchange_server.rs              # binary entry point
crates/api/tests/e2e.rs                            # 5 end-to-end tests
crates/api/PHASE5_RESULTS.md                      # this file
common/src/lib.rs                                  # unchanged (zero-dep invariant preserved)
README.md                                          # Phase 5 marked ✅
CLAUDE.md                                          # Phase 5 section + Exit criteria ✅
```

## Working-agreement compliance

- ✅ Stated phase (Phase 5) at session start
- ✅ Did not implement Postgres runtime (still a Phase 3.5 expansion); in-memory ledger is sufficient and tested
- ✅ Defaulted to less: no refresh tokens, no rate-limit-per-IP, no Prometheus metrics — just the four CLAUDE.md deliverables
- ✅ No flag needed: nothing reintroduced real custody, fiat, or KYC

## Awaiting your explicit confirmation before Phase 6 starts.