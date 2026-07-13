# Phase 2 — Concurrency Layer: Results

**Status:** ✅ exit criteria met. Awaiting explicit confirmation before Phase 3.

## Verification commands

```sh
cargo build -p actors
cargo test  -p actors --test load --release -- --nocapture --test-threads=1
cargo test  --workspace
cargo tree  -p matching-engine -e normal --depth 1   # Phase 0 invariant
```

## Test results

| Test | Result |
|---|---|
| `actor_lifecycle_smoke` | ✅ submit + best_bid + drop + task joins cleanly |
| `actor_cancel_propagates` | ✅ first cancel Ok; second cancel yields `EngineError::UnknownOrder` |
| `concurrent_flood_preserves_invariants` | ✅ 32 000 orders across 10 symbols, 64 concurrent tasks |

Full workspace (`cargo test --workspace`): all 28 matching-engine tests + 3 actor tests pass.

## Load test results (release mode)

```
=== Phase 2 load test ===
  symbols:        10
  tasks:          64
  orders/task:    500
  total orders:   32000
  wall time:      20.553485ms
  --- end-to-end round-trip latency (send -> recv) ---
  min  (ns):      373
  mean (ns):      35464
  p50  (ns):      12561
  p99  (ns):      180255
  p999 (ns):      252301
  max  (ns):      345325
  throughput:     1556914 orders/sec
```

| Metric | Value | Notes |
|---|---:|---|
| Total orders | 32 000 | 50% matched, 50% rested, evenly distributed across 10 symbols |
| Wall time | 20.5 ms | full flood including mpsc + actor loop + response back |
| Throughput | **~1.56 M orders/sec** | end-to-end (send → recv) |
| p50 latency | **12.5 µs** | typical one-way |
| p99 latency | 180 µs | tail under contention |
| p999 latency | 252 µs | worst-case tail |
| Max latency | 345 µs | single-shot extreme |

## Correctness under load

The load test asserts three things after the flood completes:

1. **Per-symbol conservation.** For each of the 10 symbols:
   `Σ submitted == Σ traded + Σ resting`. (Phase 1 invariant re-verified under concurrency.)
2. **No crossed spread.** `best_bid < best_ask` whenever both sides are non-empty, on every symbol.
3. **No silent loss.** No `EngineError::DuplicateOrderId` or other engine rejection occurred unexpectedly — every command we sent was accepted.

All three pass.

## Ordering guarantees

Per-symbol ordering is guaranteed **by construction**: each symbol has one actor processing commands serially via `mpsc` (FIFO). No mutex or shared state across actors means no chance of cross-symbol or per-symbol reordering.

The load test does not (and cannot) verify ordering via timing alone — but it verifies the *consequence* of ordering: conservation + no-crossed-spread + zero rejections. If commands were reordered in a way that mattered, conservation would be violated (different fills would happen, different orders would rest, etc).

## Architecture

```
crates/actors/
├── Cargo.toml          # depends on common + matching-engine + tokio
├── src/
│   ├── lib.rs          # re-exports
│   ├── actor.rs        # EngineActor + actor_task + ActorError
│   └── command.rs      # internal Command enum
└── tests/
    └── load.rs         # 3 tests (smoke, cancel, concurrent flood)
```

Each `EngineActor`:
- Wraps an `mpsc::Sender<Command>` (default buffer 1024; tunable via `spawn_with_buffer`).
- Owns a `MatchingEngine` inside a dedicated `tokio::spawn`'d task.
- Receives `Command`s serially, processes them, replies via per-command `oneshot::Sender`.
- Shuts down when all clones are dropped (channel closes, `recv()` returns `None`).

`EngineActor: Clone` is cheap — it's just a clone of the `mpsc::Sender` (Arc internally).

## Dependency discipline

`cargo tree -p matching-engine -e normal --depth 1`:
```
matching-engine v0.1.0
└── common v0.1.0
```

The matching engine library remains **async-free**. All tokio deps live in `actors`, which depends on `matching-engine` — never the other way around. The Phase 0 invariant is preserved.

`actors` lib tokio features: `["rt", "sync", "macros"]`. Dev features add `rt-multi-thread` + `time` for the load test.

## Files added in Phase 2

```
Cargo.toml                                 # added crates/actors to workspace
crates/actors/Cargo.toml                   # new crate
crates/actors/src/lib.rs                   # re-exports
crates/actors/src/command.rs               # internal Command enum (oneshot per cmd)
crates/actors/src/actor.rs                 # EngineActor + actor_task + ActorError
crates/actors/tests/load.rs                # 3 tests: smoke + cancel + concurrent flood
```

## What deliberately does NOT live here

- **No routing registry** (`HashMap<Symbol, EngineActor>`). Phase 5 API will own its own; this crate provides only the primitive.
- **No auth / RBAC / HTTP.** That's Phase 4/5.
- **No persistence / WAL.** That's Phase 5.
- **No balance / ledger.** That's Phase 3.
- **No retries / reconnection.** A failed `submit_limit` returns `ActorError::SendFailed` or `RecvFailed`; the caller decides what to do.

## Awaiting your explicit confirmation before Phase 3 starts.