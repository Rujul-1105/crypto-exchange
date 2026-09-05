//! Wire messages — what crosses the Redis Streams bridge.
//!
//! `OrderCommand` goes from API server → orderbook server via `orders:incoming`.
//! `EventEnvelope` is the orderbook's reply on `events:outgoing`.
//! `SettleUpdate` is the settler's reply on `settle:updates`.

use crate::events::*;
use crate::order::*;
use crate::side::*;
use crate::trade::*;
use serde::{Deserialize, Serialize};

/// What the API server pushes onto `orders:incoming`.
///
/// We use a tagged enum so the consumer can deserialise directly into the
/// concrete command type. The wire field is `op`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum OrderCommand {
    Place(PlaceOrder),
    Cancel(CancelOrder),
    Amend(AmendOrder),
}

impl OrderCommand {
    /// Symbol the command targets. Used by the orderbook's Redis consumer to
    /// pick the right engine out of the registry.
    pub fn symbol(&self) -> &Symbol {
        match self {
            OrderCommand::Place(p) => &p.symbol,
            OrderCommand::Cancel(c) => &c.symbol,
            OrderCommand::Amend(a) => &a.symbol,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaceOrder {
    pub user: String,
    pub symbol: Symbol,
    pub side: Side,
    #[serde(flatten)]
    pub order_type: OrderType,
    pub tif: TimeInForce,
    pub price: Option<Price>,
    pub quantity: Quantity,
    /// Set when the API server hands the order a tentative id; the engine may
    /// reuse its own monotonic counter and ignore this. Useful for clients that
    /// want to track pending -> accepted handoff.
    pub client_order_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CancelOrder {
    pub user: String,
    pub symbol: Symbol,
    pub order_id: OrderId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AmendOrder {
    pub user: String,
    pub symbol: Symbol,
    pub order_id: OrderId,
    pub new_price: Price,
    pub new_quantity: Quantity,
}

/// Envelope wrapping an EngineEvent with the Redis stream id assigned by the
/// producer. The id is needed by the settler worker (to be idempotent) and by
/// the snapshot/replay logic (to know what has been published).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub stream_id: String,
    pub symbol: Symbol,
    pub event: EngineEvent,
}

impl EventEnvelope {
    pub fn new(stream_id: impl Into<String>, symbol: Symbol, event: EngineEvent) -> Self {
        Self {
            stream_id: stream_id.into(),
            symbol,
            event,
        }
    }
}

/// Settler reply on `settle:updates`. Consumed by the orderbook to flip
/// `Trade.settle_status` and rebroadcast as a Trade event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SettleUpdate {
    pub trade_id: TradeId,
    pub symbol: Symbol,
    pub status: SettleStatus,
    pub signature: Option<String>,
    pub error: Option<String>,
}
