//! Trade — emitted on every fill.

use crate::order::*;
use crate::side::*;
use serde::{Deserialize, Serialize};

pub type TradeId = u64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Trade {
    pub id: TradeId,
    pub symbol: Symbol,
    pub price: Price,
    pub quantity: Quantity,
    pub buy_order_id: OrderId,
    pub sell_order_id: OrderId,
    pub taker_side: Side,
    pub buyer: String,
    pub seller: String,
    pub timestamp: Timestamp,
    pub settle_status: SettleStatus,
}

impl Trade {
    /// Notional in quote currency (price * quantity). For SOL/USDC, USDC.
    pub fn notional(&self) -> Quantity {
        self.price * self.quantity
    }
}
