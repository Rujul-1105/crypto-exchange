//! Order book data structure — `BTreeMap<Price, VecDeque<OrderId>>` per side
//! plus a `HashMap<OrderId, Order>` for O(1) lookup. Quantity at each level
//! is computed on demand from the orders map (simpler than maintaining a
//! running sum that has to be updated on every partial fill).

use common::*;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// One side of the book. Bids are stored ascending by price (best bid is
/// `last_key_value`); asks ascending (best ask is `first_key_value`).
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookSide {
    /// price → FIFO queue of order ids resting at that price
    pub levels: BTreeMap<Price, VecDeque<OrderId>>,
}

impl BookSide {
    pub fn best_bid(
        levels: &BTreeMap<Price, VecDeque<OrderId>>,
    ) -> Option<(Price, &VecDeque<OrderId>)> {
        levels.iter().next_back().map(|(p, q)| (*p, q))
    }
    pub fn best_ask(
        levels: &BTreeMap<Price, VecDeque<OrderId>>,
    ) -> Option<(Price, &VecDeque<OrderId>)> {
        levels.iter().next().map(|(p, q)| (*p, q))
    }

    /// Sum the remaining qty at `price` by walking the orders map.

    // Calculates the total remaining quantity at a given price level.
    // Finds all order IDs at that price, looks up their orders, sums each order's remaining quantity and returns 0 if the level doesn't exist.
    pub fn level_qty(&self, price: Price, orders: &HashMap<OrderId, Order>) -> Quantity {
        self.levels
            .get(&price)
            .map(|q| {
                q.iter()
                    .filter_map(|oid| orders.get(oid))
                    .map(|o| o.remaining())
                    .fold(Quantity::ZERO, |acc, q| acc + q)
            })
            .unwrap_or_default()
    }
}

/// The order book for one symbol.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderBook {
    pub symbol: Symbol,
    pub bids: BookSide,
    pub asks: BookSide,
    pub orders: HashMap<OrderId, Order>,
    pub last_trade_price: Option<Price>,
}

impl OrderBook {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            bids: BookSide::default(),
            asks: BookSide::default(),
            orders: HashMap::new(),
            last_trade_price: None,
        }
    }

    /// Best bid (highest buy price) and best ask (lowest sell price).
    pub fn top_of_book(&self) -> (Option<(Price, Quantity)>, Option<(Price, Quantity)>) {
        let bid = BookSide::best_bid(&self.bids.levels)
            .map(|(p, _)| (p, self.bids.level_qty(p, &self.orders)));
        let ask = BookSide::best_ask(&self.asks.levels)
            .map(|(p, _)| (p, self.asks.level_qty(p, &self.orders)));
        (bid, ask)
    }

    /// Top `levels` bids (descending) and asks (ascending), each as
    /// `(price, total_remaining_qty)` vectors.
    pub fn depth_snapshot(&self, levels: usize) -> DepthSnapshot {
        let bids: Vec<(Price, Quantity)> = self
            .bids
            .levels
            .iter()
            .rev()
            .take(levels)
            .map(|(p, _)| (*p, self.bids.level_qty(*p, &self.orders)))
            .collect();

        let asks: Vec<(Price, Quantity)> = self
            .asks
            .levels
            .iter()
            .take(levels)
            .map(|(p, _)| (*p, self.asks.level_qty(*p, &self.orders)))
            .collect();

        DepthSnapshot {
            symbol: self.symbol.clone(),
            bids,
            asks,
            last_trade_price: self.last_trade_price,
        }
    }

    /// Insert an order at the back of its price level. Caller must validate
    /// `price` is Some for limit-like orders.
    pub fn insert(&mut self, order: Order) {
        let side = match order.side {
            Side::Buy => &mut self.bids.levels,
            Side::Sell => &mut self.asks.levels,
        };
        let price = order.price.expect("limit order must have price");
        side.entry(price).or_default().push_back(order.id);
        self.orders.insert(order.id, order);
    }

    /// Cancel an order by id, removing it from the level queue. Returns the
    /// removed order.
    pub fn cancel(&mut self, id: OrderId) -> Option<Order> {
        let order = self.orders.remove(&id)?;
        let side = match order.side {
            Side::Buy => &mut self.bids.levels,
            Side::Sell => &mut self.asks.levels,
        };
        if let Some(price) = order.price {
            if let Some(queue) = side.get_mut(&price) {
                queue.retain(|oid| *oid != id);
                if queue.is_empty() {
                    side.remove(&price);
                }
            }
        }
        Some(order)
    }

    /// Lookup without removing.
    pub fn get(&self, id: OrderId) -> Option<&Order> {
        self.orders.get(&id)
    }

    /// Mutable lookup (used by the engine to mutate `filled` mid-fill).
    pub fn get_mut(&mut self, id: OrderId) -> Option<&mut Order> {
        self.orders.get_mut(&id)
    }

    /// Number of open orders in the book.
    pub fn open_order_count(&self) -> usize {
        self.orders.len()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DepthSnapshot {
    pub symbol: Symbol,
    pub bids: Vec<(Price, Quantity)>,
    pub asks: Vec<(Price, Quantity)>,
    pub last_trade_price: Option<Price>,
}
