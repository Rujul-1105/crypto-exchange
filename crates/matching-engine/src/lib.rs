//! The matching engine — the centerpiece of the exchange.
//!
//! ## Phase 0 status
//!
//! This crate exists as a workspace member with a compileable stub so the
//! crate boundary is real, not aspirational. The dependency graph is the
//! Phase 0 exit criterion: this crate depends only on `common` (and
//! transitively on `std`), so it is usable from a plain `#[test]` with no
//! tokio / async runtime and no database.
//!
//! ## What goes here in Phase 1
//!
//! - `Order` struct, `Book` structure (`BTreeMap<Price, VecDeque<Order>>`)
//! - Operations: `submit_limit`, `submit_market`, `cancel`,
//!   `best_bid`, `best_ask`, `snapshot`
//! - Strict price-time priority matching
//! - Property-based tests with `proptest` (invariants: price-time priority,
//!   quantity conservation, no crossed spread)
//! - `criterion` benchmarks for orders/sec throughput and p50/p99 latency
//!
//! Do not introduce async, DB, or HTTP types into this crate to "save a
//! hop" from the API layer. Keep this crate deterministic and dependency-
//! light; that's what makes it trustworthy for money.

#![doc = "Phase 0 placeholder. Real API arrives in Phase 1."]

/// Placeholder type so the crate has something to export. Will be replaced
/// in Phase 1 by `Order`, `Book`, `MatchResult`, etc.
#[derive(Debug, Clone, Copy)]
pub struct MatchingEngine;

impl MatchingEngine {
    /// Construct a new engine. Phase 1 will thread configuration through
    /// here (symbol, tick size, etc.).
    pub fn new() -> Self {
        Self
    }
}

impl Default for MatchingEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase 0 smoke test: prove the crate is usable from a plain `#[test]`
    /// with no runtime. If this test ever requires `#[tokio::test]` or an
    /// async context, the crate boundary has been violated.
    #[test]
    fn engine_constructs_in_plain_test() {
        let _engine = MatchingEngine::new();
    }
}