# Project: Rust Custodial-Simulation Crypto Exchange

## Scope (read this before touching any phase)

This is a **portfolio project**. It simulates a centralized exchange (CEX) internally:
- **No real wallets, no real keys, no on-chain custody.** Balances are internal ledger entries only.
- **No fiat rails, no real KYC, no cross-exchange liquidity.** Out of scope, permanently.
- The point of this project is to demonstrate: (1) a correct, fast, well-tested matching engine,
  (2) a correct double-entry ledger with atomic settlement, (3) reasonable concurrency design,
  (4) standard secure API/auth practices. In that priority order.

**Non-negotiable rule for Claude Code: do not start Phase N+1 work until Phase N's exit criteria
(listed below) are met and I have explicitly confirmed it.** If asked to "just also stub out the
API while we're in Phase 1," refuse and remind me of this rule. The most common failure mode in
projects like this is the order book ending up as a thin, undertested layer buried inside a CRUD
app — do not let that happen.

---

## Phase 0 — Scope Lock & Repo Setup

**Goal:** project skeleton exists, decisions are written down, nothing else.

- [ ] Cargo workspace with separate crates: `matching-engine` (pure lib, no async/db deps),
      `ledger`, `api`, `common` (shared types: OrderId, Price, Qty, Symbol, etc.)
- [ ] Top-level `frontend/` directory for the Phase 7 Next.js SPA (outside the Cargo
      workspace, documented in Phase 7's section below)
- [ ] `matching-engine` crate has **zero** dependencies on tokio, sqlx, axum, or anything async.
      It must be usable from a plain `#[test]` with no runtime.
- [ ] README stating the non-goals above, so future-me (or a reviewer) knows this was deliberate.

**Exit criteria:** `cargo build` passes on an empty workspace with the crate boundaries above.
Do not proceed to Phase 1 until this structure exists.

---

## Phase 1 — Order Book Core (the centerpiece)

**Goal:** a correct, benchmarked, fully isolated matching engine. No DB, no network, no async.

- [ ] Core types: `Order { id, side, price, qty, timestamp, kind: Limit|Market }`
- [ ] Book structure: `BTreeMap<Price, VecDeque<Order>>` per side (bids descending, asks ascending)
- [ ] Operations: `submit_limit`, `submit_market`, `cancel`, `best_bid`, `best_ask`, `snapshot`
- [ ] Matching logic: strict price-time priority, partial fills, market orders sweep the book
      until filled or book exhausted (define behavior on partial-fill-then-exhausted explicitly)
- [ ] Property-based tests (proptest) asserting invariants:
      - price-time priority is never violated
      - total quantity is conserved across any sequence of fills/cancels (no phantom liquidity)
      - book never produces a crossed spread after matching completes
- [ ] Criterion benchmarks: orders/sec throughput, p50/p99 latency for submit + match
- [ ] Unit tests for edge cases: self-crossing orders, zero-qty rejection, cancel-after-partial-fill

**Exit criteria:** `cargo test -p matching-engine` and `cargo bench -p matching-engine` both pass
and produce a written summary (throughput numbers, invariants tested). Show me the results before
Phase 2 starts.

---

## Phase 2 — Concurrency Layer

**Goal:** wrap the (already-correct) book in a concurrency model without touching its internals.

- [ ] Single-writer-per-symbol actor model: one dedicated thread/task owns each symbol's book,
      receives commands via `mpsc` channel, replies via oneshot or response channel.
- [ ] No shared mutable state across symbol actors — do not introduce a global lock or `DashMap`
      of books unless a specific measured bottleneck justifies it later.
- [ ] Load test: synthetic concurrent order flood across multiple symbols, measure end-to-end
      latency distribution and confirm no ordering violations under load.

**Exit criteria:** load test results written down (throughput under concurrent load, any
correctness regressions vs Phase 1 invariants re-run under concurrency).

---

## Phase 3 — Ledger (Double-Entry, Atomic Settlement)

**Goal:** correct internal balance accounting. This is a correctness problem, treat it like one.

- [ ] Account model: `available_balance` and `locked_balance` per (user, asset) — never a single
      mutable balance field.
- [ ] Placing an order locks funds (move available → locked) before it reaches the book.
- [ ] A trade fill is a single atomic DB transaction: debit/credit both sides' locked/available
      balances, mark order status, insert trade record, insert ledger entries — all or nothing.
- [ ] Explicit test for the double-spend trap: user has 100 USDC, places two orders that would
      each individually be affordable but together are not — second order must be rejected at
      lock time, not after both match.
- [ ] Postgres schema for accounts, ledger_entries (append-only), orders, trades.

**Exit criteria:** integration tests proving atomicity (simulate a crash mid-transaction if
feasible, or at minimum assert no partial-write state is ever observable) and the double-spend
test above passes.

---

## Phase 4 — Auth & RBAC

**Goal:** standard, low-risk web auth. Do not over-invest time here relative to Phase 1/3.

- [ ] Argon2 password hashing, JWT-based session auth
- [ ] Roles: `user`, `market_maker`, `admin` — enforced via middleware/extractor, not scattered
      `if role == admin` checks inline in handlers
- [ ] Admin-only endpoints: manual balance adjustment (for demo/testing), symbol management

**Exit criteria:** auth middleware has tests for each role's access boundaries (403 on
unauthorized role, 200 on authorized).

---

## Phase 5 — REST API & Persistence Integration

**Goal:** wire the already-tested engine, ledger, and auth together. This phase should feel like
plumbing, not discovery — if you're debugging matching logic here, Phase 1 wasn't actually done.

- [ ] Axum (or actix-web) REST API: place order, cancel order, get book snapshot, get balances,
      get trade history
- [ ] Idempotency keys on order submission (dedupe retried requests)
- [ ] Per-user rate limiting
- [ ] Event log / WAL for the in-memory book: on restart, replay persisted events to reconstruct
      book state (this is what makes "persistent storage" meaningful rather than decorative)

**Exit criteria:** end-to-end test: submit orders via HTTP, confirm trade execution, confirm
balances update correctly, restart the process and confirm book state recovers from the event log.

---

## Phase 6 — Hardening & Writeup

**Goal:** the artifact a reviewer or hiring manager actually reads.

- [ ] End-to-end load test (not just Phase 1's isolated benchmark)
- [ ] Written doc: the specific bug classes this design prevents and how (double-spend, phantom
      fills, non-atomic settlement, crossed book) — this matters more than any additional feature
- [ ] Explicit list of what was deliberately left out and why (real custody, fiat, KYC)

**Exit criteria:** doc exists, load test numbers exist, project is demo-ready.

---

## Phase 7 — Frontend (Next.js SPA)

**Goal:** a separate Next.js single-page app that consumes the Phase 5
Axum REST API, giving reviewers a visual UI to place orders, watch the
book, and inspect balances / trade history. Lives outside the Rust
workspace in `frontend/` at the repo root, since Next.js brings its own
Node.js toolchain and mixing it into the Cargo workspace would add noise
without benefit.

- [ ] Next.js 14+ with App Router + TypeScript
- [ ] Auth: JWT in `Authorization: Bearer …`, stored in an httpOnly cookie
      set by the API (Phase 4's session token)
- [ ] Pages: login / register, dashboard with balances, order-entry form,
      order book view, trade history
- [ ] Server state via TanStack Query (or SWR); polling for now — no
      WebSocket yet (deferred to a later minor phase)
- [ ] REST client in `src/lib/api.ts` — single source of truth for
      endpoint paths and types, mirroring the Phase 5 OpenAPI schema
- [ ] Component-level tests with React Testing Library (optional but
      encouraged for any non-trivial view)

**Exit criteria:** `cd frontend && npm install && npm run dev` starts
the dev server; `npm run build` produces a working static or SSR export;
an end-to-end happy path (register → log in → view book → place a limit
order → see fill → view updated balance) works against a locally-running
Phase 5 server. README points at the frontend; this section exists.

---

## Working agreement for Claude Code sessions

1. Always state which phase we're in at the start of a session.
2. Do not write API/DB/auth code while Phase 1 or Phase 2 invariants are unverified.
3. When in doubt about scope, default to *less* — this is a demonstration project, not a product.
4. Flag if a task implicitly reintroduces real custody, fiat, or KYC — those are permanently
   out of scope per the Scope section above.