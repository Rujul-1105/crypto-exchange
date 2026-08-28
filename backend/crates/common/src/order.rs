//! Order struct and OrderId counter.

use crate::side::*;
use serde::{Deserialize, Serialize};

pub type OrderId = u64;
pub type Symbol = String;
pub type Timestamp = i64;

/// An order as it lives in the matching engine.
///
/// The same `Order` type is emitted inside `EngineEvent` so WS clients and
/// history consumers don't need a second representation. We re-emit the order
/// with updated `filled`/`status` on each lifecycle event (Accepted, Fill,
/// Cancelled, Amended).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    /// Wallet pubkey as base58 string. On the matching layer we don't care
    /// about the curve — the settler worker takes care of signing.
    pub user: String,
    pub symbol: Symbol,
    pub side: Side,
    #[serde(flatten)]
    pub order_type: OrderType,
    pub tif: TimeInForce,
    /// Required for Limit + StopLimit; absent for Market + Stop.
    pub price: Option<Price>,
    pub quantity: Quantity,
    pub filled: Quantity,
    pub status: OrderStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Order {
    /// Remaining quantity that has not yet been filled.
    pub fn remaining(&self) -> Quantity {
        self.quantity - self.filled
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status, OrderStatus::New | OrderStatus::PartiallyFilled)
    }
}

/// Per-engine monotonic ID generator.
#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderIdGen {
    next: OrderId,
}

impl OrderIdGen {
    pub fn new(start: OrderId) -> Self {
        Self { next: start }
    }
    pub fn next(&mut self) -> OrderId {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_id_gen_starts_at_zero_by_default() {
        let mut g = OrderIdGen::default();
        assert_eq!(g.next(), 0);
        assert_eq!(g.next(), 1);
        assert_eq!(g.next(), 2);
    }

    #[test]
    fn order_id_gen_respects_start() {
        let mut g = OrderIdGen::new(100);
        assert_eq!(g.next(), 100);
        assert_eq!(g.next(), 101);
    }

    #[test]
    fn order_remaining() {
        use rust_decimal_macros::dec;
        let order = Order {
            id: 1,
            user: "test".into(),
            symbol: "SOL-USDC".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            tif: TimeInForce::Gtc,
            price: Some(dec!(100)),
            quantity: dec!(10),
            filled: dec!(3),
            status: OrderStatus::PartiallyFilled,
            created_at: 0,
            updated_at: 0,
        };
        assert_eq!(order.remaining(), dec!(7));
        assert!(order.is_open());
    }
}
