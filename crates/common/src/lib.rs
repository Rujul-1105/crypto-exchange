//! Shared types for the exchange.
//!
//! This crate is intentionally leaf-level: it has zero dependencies so it can
//! be used from every other crate (matching-engine, ledger, api) without
//! dragging in any runtime, database, or web concerns.
//!
//! Phase 0 defined the type skeletons. Phase 1 adds [`Order`] — the core
//! order struct used by the matching engine, and (later) by the ledger and
//! API layers.

use std::fmt;
use std::ops::{Add, Sub};

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

impl Side {
    /// The opposite side.
    pub fn opposite(self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Qty(pub u64);

impl Add for Qty {
    type Output = Qty;
    fn add(self, rhs: Qty) -> Qty {
        Qty(self.0 + rhs.0)
    }
}

impl Sub for Qty {
    type Output = Qty;
    fn sub(self, rhs: Qty) -> Qty {
        Qty(self.0 - rhs.0)
    }
}

impl Qty {
    pub const ZERO: Qty = Qty(0);

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// Monotonic timestamp in nanoseconds since process start.
///
/// Phase 0 uses a placeholder wall-clock source; the matching engine will
/// own time for price-time priority and inject this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Timestamp(pub u64);

/// A resting or incoming order on the matching engine.
///
/// `qty` is the **remaining** quantity, not the original placement size.
/// The matching engine decrements it as fills occur. To recover the original
/// placement size, sum `qty` plus the trade qtys attributed to this order.
///
/// `price` is `Some(_)` for limit orders and `None` for market orders.
/// The matching engine validates this pairing in `submit_*` and rejects
/// mismatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    pub price: Option<Price>,
    pub qty: Qty,
    pub timestamp: Timestamp,
    pub kind: OrderKind,
}

impl Order {
    /// Construct a limit order. Engine-side validation is the source of
    /// truth for `qty > 0`; this constructor just builds the struct with
    /// the kind/price pairing locked in.
    pub fn limit(
        id: OrderId,
        side: Side,
        price: Price,
        qty: Qty,
        timestamp: Timestamp,
    ) -> Self {
        Order {
            id,
            side,
            price: Some(price),
            qty,
            timestamp,
            kind: OrderKind::Limit,
        }
    }

    /// Construct a market order. Engine-side validation is the source of
    /// truth for `qty > 0`.
    pub fn market(id: OrderId, side: Side, qty: Qty, timestamp: Timestamp) -> Self {
        Order {
            id,
            side,
            price: None,
            qty,
            timestamp,
            kind: OrderKind::Market,
        }
    }
}

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
        assert_eq!(Side::Buy.opposite(), Side::Sell);
        assert_eq!(Side::Sell.opposite(), Side::Buy);

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

    #[test]
    fn qty_arithmetic() {
        assert_eq!(Qty(3) + Qty(4), Qty(7));
        assert_eq!(Qty(10) - Qty(3), Qty(7));
        assert!(Qty::ZERO.is_zero());
        assert!(!Qty(1).is_zero());
    }

    #[test]
    fn order_constructors_set_kind_and_price() {
        let lim = Order::limit(OrderId(1), Side::Buy, Price(100), Qty(5), Timestamp(0));
        assert_eq!(lim.kind, OrderKind::Limit);
        assert_eq!(lim.price, Some(Price(100)));

        let mkt = Order::market(OrderId(2), Side::Sell, Qty(5), Timestamp(0));
        assert_eq!(mkt.kind, OrderKind::Market);
        assert_eq!(mkt.price, None);
    }
}