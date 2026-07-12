//! Shared types for the exchange.
//!
//! This crate is intentionally leaf-level: it has zero dependencies so it can
//! be used from every other crate (matching-engine, ledger, api) without
//! dragging in any runtime, database, or web concerns.
//!
//! Phase 0 only defines the type skeletons. Semantic meaning (price
//! representation, order kinds, timestamps) is locked in here so that later
//! phases can build on a stable foundation.

use std::fmt;

/// A unique identifier for an order.
///
/// In Phase 0 this is an opaque wrapper; later phases will decide whether
/// it carries a creation timestamp, a per-symbol sequence number, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderId(pub u64);

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ord#{}", self.0)
    }
}

/// A unique identifier for a settled trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TradeId(pub u64);

/// Order side: buy (bid) or sell (ask).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

/// Order kind. Market orders sweep the book until filled or exhausted;
/// limit orders rest on the book at a specified price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderKind {
    Limit,
    Market,
}

/// Trading symbol (e.g. "BTC-USDC").
///
/// Stored as a small string for Phase 0. The matching engine will use this
/// as the partition key for symbol actors in Phase 2.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol(pub String);

impl Symbol {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Symbol {
    fn from(s: &str) -> Self {
        Symbol(s.to_owned())
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Price, represented as integer minor units (e.g. USDC micros = 1e-6 USDC).
///
/// Using integer minor units avoids floating-point drift in the matching
/// engine. The scale factor (10^6 here) is fixed at the crate boundary so
/// no rounding errors can leak across module boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Price(pub u64);

/// Quantity, represented as integer minor units (e.g. BTC satoshis = 1e-8 BTC).
///
/// Same rationale as [`Price`]: integer-only arithmetic throughout the
/// engine keeps fill math exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Qty(pub u64);

/// Monotonic timestamp in nanoseconds since process start.
///
/// Phase 0 uses a placeholder wall-clock source; the matching engine will
/// own time for price-time priority and inject this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_round_trip() {
        let id = OrderId(42);
        assert_eq!(id.0, 42);
        assert_eq!(format!("{}", id), "ord#42");

        let sym = Symbol::from("BTC-USDC");
        assert_eq!(sym.as_str(), "BTC-USDC");
        assert_eq!(format!("{}", sym), "BTC-USDC");

        assert_eq!(Side::Buy, Side::Buy);
        assert_ne!(Side::Buy, Side::Sell);

        assert_eq!(OrderKind::Limit, OrderKind::Limit);
        assert_ne!(OrderKind::Limit, OrderKind::Market);
    }

    #[test]
    fn numeric_types_are_ordered() {
        // Price and Qty must be Ord so BTreeMap<Price, ...> works in the
        // matching engine. Assert that here so future refactors can't break
        // this invariant silently.
        assert!(Price(100) < Price(200));
        assert!(Qty(1) < Qty(2));
        assert!(Timestamp(1) < Timestamp(2));
    }
}