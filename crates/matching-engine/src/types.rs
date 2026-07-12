//! Public types returned by the matching engine.
//!
//! These describe the outcome of a submit or the state of the book. They
//! are deliberately plain data — no behavior, no async, no I/O.

use common::{OrderId, Price, Qty};

/// The outcome of a single submit (limit or market).
///
/// `trades` lists every fill that occurred, in execution order.
/// `resting_order_id` is `Some` only when a limit taker partially filled
/// and the remainder was placed on the book.
/// `cancelled_remainder_qty` is non-zero only for market orders that
/// couldn't be fully filled against available liquidity (the unfilled
/// remainder is silently cancelled — this is the standard CEX behavior
/// for market orders, and matches Binance/Coinbase/Kraken).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchResult {
    pub trades: Vec<Trade>,
    pub resting_order_id: Option<OrderId>,
    pub cancelled_remainder_qty: Qty,
}

/// A single fill between a resting maker and an incoming taker.
///
/// Per price-time priority, the trade price is the **maker's** (resting)
/// price. The taker accepts that price in exchange for execution certainty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Trade {
    pub maker_order_id: OrderId,
    pub taker_order_id: OrderId,
    pub price: Price,
    pub qty: Qty,
}

/// Aggregated view of the book at a point in time.
///
/// `bids` is sorted by price descending; `asks` by price ascending. Each
/// entry sums the remaining quantities at that price level.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub bids: Vec<(Price, Qty)>,
    pub asks: Vec<(Price, Qty)>,
}

/// Errors returned by submit/cancel operations. None of these mutate the
/// book; on error the engine is in the same state as before the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineError {
    /// Order qty was zero. Orders must request at least one unit.
    ZeroQuantity,
    /// Limit order was submitted without a price.
    LimitOrderWithoutPrice,
    /// Market order was submitted with a price (should be `None`).
    MarketOrderWithPrice,
    /// Cancel targeted an order id that is not currently resting on the book.
    /// Either it was never submitted, was already cancelled, or was fully
    /// filled and removed.
    UnknownOrder,
    /// Submit targeted an order id that is already resting on the book.
    /// In real exchanges order ids are globally unique per session; the
    /// engine enforces the same invariant. After a cancel or full fill
    /// the id becomes available for reuse.
    DuplicateOrderId,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::ZeroQuantity => f.write_str("order qty must be > 0"),
            EngineError::LimitOrderWithoutPrice => {
                f.write_str("limit order must have a price")
            }
            EngineError::MarketOrderWithPrice => {
                f.write_str("market order must not have a price")
            }
            EngineError::UnknownOrder => f.write_str("order id is not resting on the book"),
            EngineError::DuplicateOrderId => f.write_str("order id is already resting on the book"),
        }
    }
}

impl std::error::Error for EngineError {}