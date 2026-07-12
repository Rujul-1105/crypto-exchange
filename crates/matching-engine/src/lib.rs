//! The matching engine — the centerpiece of the exchange.
//!
//! ## Phase 1 scope
//!
//! Pure-library implementation of a limit/market order book with strict
//! price-time priority, partial fills, cancellation, and snapshots. All
//! arithmetic is integer minor units; no floats anywhere. Designed to be
//! usable from a plain `#[test]` with no runtime, no database, and no
//! network — that constraint is enforced by the absence of async/db/web
//! dependencies in `Cargo.toml`.
//!
//! ## What deliberately does NOT live here
//!
//! - Concurrency / actors (Phase 2 wraps this in a per-symbol task).
//! - Persistence / event log / WAL (Phase 5).
//! - Auth / RBAC / HTTP (Phase 4/5).
//! - Balances / settlement (Phase 3, ledger).
//!
//! Keep this crate deterministic and dependency-light. If a feature seems
//! to need `tokio` or `sqlx`, it almost certainly belongs in another crate.

mod engine;
mod types;

pub use engine::MatchingEngine;
pub use types::{EngineError, MatchResult, Snapshot, Trade};

// Re-export `Order` from common for convenience, since almost every caller
// of this crate builds orders directly.
pub use common::Order;