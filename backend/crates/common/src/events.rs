//! EngineEvent — everything the matching engine emits per order/cancel/amend.
//!
//! Consumers (Redis publisher, WS broadcaster, storage) consume these as
//! `Vec<EngineEvent>` and dispatch.

use crate::candle::*;
use crate::order::*;
use crate::side::*;
use crate::trade::*;
use serde::{Deserialize, Serialize};

// Re-export for ergonomics inside this module's signatures.
// (The imports above pull in everything; explicit aliases below avoid the
// common gotcha where a type alias defined in one module isn't auto-resolved
// from another.)

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    /// Order has been admitted to the book (or rejected).
    Accepted { order: Order },
    /// Order was rejected before admission (e.g. invalid params).
    Rejected {
        id: OrderId,
        reason: String,
        symbol: Symbol,
        user: String,
    },
    /// One fill has occurred. The matching loop emits one `Fill` per matched
    /// resting order, so a single taker can produce multiple `Fill` events.
    Fill {
        trade: Trade,
        maker: Order,
        taker: Order,
    },
    /// Convenience: same data as `Fill` but without the maker/taker copies.
    /// The engine emits exactly one `Trade` per `Fill`; clients that only want
    /// the trade tape can subscribe to this and skip `Fill`.
    Trade { trade: Trade },
    /// Top-of-book or level size changed. `new_qty == 0` means the level is gone.
    BookDelta {
        symbol: Symbol,
        side: Side,
        price: Price,
        new_qty: Quantity,
        ts: Timestamp,
    },
    /// Candle update. `is_closed=true` only when the bucket flipped.
    CandleUpdate {
        symbol: Symbol,
        candle: Candle,
        is_closed: bool,
    },
    /// Order was cancelled by user request.
    Cancelled {
        id: OrderId,
        symbol: Symbol,
        user: String,
        remaining: Quantity,
    },
    /// Order was replaced (price/quantity changed; loses time priority).
    Amended { id: OrderId, order: Order },
    /// Settlement status flipped (consumed from `settle:updates`). The
    /// engine rebroadcasts it on `events:outgoing` so WS clients learn
    /// when a `Trade` was confirmed/failed on-chain.
    SettleUpdate {
        trade_id: TradeId,
        symbol: Symbol,
        status: SettleStatus,
        signature: Option<String>,
        error: Option<String>,
    },
}
