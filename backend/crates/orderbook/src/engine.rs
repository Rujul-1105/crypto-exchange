//! Matching engine — pure, no I/O.
//!
//! - `match_entry(order, now)` — admit an order, match as much as possible at
//!   resting prices, rest the remainder if limit, drop if market-IOC. Returns
//!   a `Vec<EngineEvent>` describing everything that happened.
//! - `cancel(id, now)` — remove an open order. Returns the events.
//! - `amend(id, new_qty, new_price, now)` — replace an order; the new order
//!   goes to the back of its (possibly new) level, losing time priority.
//! - `trigger_stops(last_trade_price, now)` — fire any triggered stops.
//!
//! The engine is the single source of truth for the book. Side effects (Redis
//! publish, WS broadcast, SQLite persistence) live in callers that consume
//! the returned events.

use crate::candle::CandleAggregator;
use crate::orderbook::DepthSnapshot;
use crate::orderbook::OrderBook;
use common::*;
use std::collections::{BTreeMap, HashMap, VecDeque};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MatchingEngine {
    pub symbol: Symbol,
    pub book: OrderBook,
    /// Trigger price → FIFO queue of stop order ids.
    pub stop_queue: BTreeMap<Price, VecDeque<OrderId>>,
    /// Lookup: order id → full Order (so we can reconstruct on trigger).
    pub stop_orders: HashMap<OrderId, Order>,
    pub order_id_gen: OrderIdGen,
    pub trade_id_gen: TradeIdGen,
    pub candle_aggregator: CandleAggregator,
    /// In-memory ring of the last N trades for `GET /api/trades/:symbol`.
    pub recent_trades: VecDeque<Trade>,
}

const RECENT_TRADES_CAP: usize = 1000;

impl MatchingEngine {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol: symbol.clone(),
            book: OrderBook::new(symbol.clone()),
            stop_queue: BTreeMap::new(),
            stop_orders: HashMap::new(),
            order_id_gen: OrderIdGen::default(),
            trade_id_gen: TradeIdGen::default(),
            candle_aggregator: CandleAggregator::new(symbol.clone()),
            recent_trades: VecDeque::with_capacity(RECENT_TRADES_CAP),
        }
    }

    pub fn push_recent_trade(&mut self, t: Trade) {
        if self.recent_trades.len() >= RECENT_TRADES_CAP {
            self.recent_trades.pop_front();
        }
        self.recent_trades.push_back(t);
    }

    // ── Public surface ──────────────────────────────────────

    pub fn top_of_book(&self) -> (Option<(Price, Quantity)>, Option<(Price, Quantity)>) {
        self.book.top_of_book()
    }

    pub fn depth_snapshot(&self, levels: usize) -> DepthSnapshot {
        self.book.depth_snapshot(levels)
    }

    pub fn open_order_count(&self) -> usize {
        self.book.open_order_count()
    }

    /// Cancel an open order. Checks both the regular book and stop queues.
    pub fn cancel(&mut self, id: OrderId, now: Timestamp) -> Vec<EngineEvent> {
        let mut events = Vec::new();

        // 1. Try cancel from the regular book.
        if let Some(mut order) = self.book.cancel(id) {
            order.status = OrderStatus::Cancelled;
            order.updated_at = now;
            let remaining = order.remaining();
            let price = order.price;
            events.push(EngineEvent::Cancelled {
                id: order.id,
                symbol: order.symbol.clone(),
                user: order.user.clone(),
                remaining,
            });
            if let Some(p) = price {
                events.push(EngineEvent::BookDelta {
                    symbol: self.symbol.clone(),
                    side: order.side,
                    price: p,
                    new_qty: Quantity::ZERO,
                    ts: now,
                });
            }
            return events;
        }

        // 2. Try cancel from stop orders.
        if let Some(mut order) = self.stop_orders.remove(&id) {
            let trigger = match &order.order_type {
                OrderType::Stop { trigger } => *trigger,
                OrderType::StopLimit { trigger, .. } => *trigger,
                _ => Price::ZERO,
            };
            if let Some(q) = self.stop_queue.get_mut(&trigger) {
                q.retain(|oid| *oid != id);
                if q.is_empty() {
                    self.stop_queue.remove(&trigger);
                }
            }
            order.status = OrderStatus::Cancelled;
            order.updated_at = now;
            let remaining = order.remaining();
            events.push(EngineEvent::Cancelled {
                id: order.id,
                symbol: order.symbol.clone(),
                user: order.user.clone(),
                remaining,
            });
        }
        events
    }

    /// Amend an open order — cancel + replace. Loses time priority (new order
    /// goes to back of its level).
    pub fn amend(
        &mut self,
        id: OrderId,
        new_qty: Quantity,
        new_price: Price,
        now: Timestamp,
    ) -> Vec<EngineEvent> {
        let prev = self
            .book
            .get(id)
            .cloned()
            .or_else(|| self.stop_orders.get(&id).cloned());
        let Some(prev) = prev else {
            return vec![EngineEvent::Rejected {
                id,
                reason: "order not found".into(),
                symbol: self.symbol.clone(),
                user: String::new(),
            }];
        };
        // Cancel existing (drops from book or stop queue).
        let cancel_events = self.cancel(id, now);
        let mut events = cancel_events;
        // Re-place at the new price/qty with same id.
        let new_order = Order {
            id,
            user: prev.user.clone(),
            symbol: prev.symbol.clone(),
            side: prev.side,
            order_type: OrderType::Limit,
            tif: prev.tif,
            price: Some(new_price),
            quantity: new_qty,
            filled: Quantity::ZERO,
            status: OrderStatus::New,
            created_at: now,
            updated_at: now,
        };
        let mut new_events = self.match_entry(new_order, now);
        // Convert the leading `Accepted` event into `Amended` so clients see it.
        for ev in &mut new_events {
            if let EngineEvent::Accepted { order } = ev {
                *ev = EngineEvent::Amended {
                    id: order.id,
                    order: order.clone(),
                };
            }
        }
        events.extend(new_events);
        events
    }

    /// Admit an order. Returns all events the engine emitted.
    pub fn match_entry(&mut self, mut order: Order, now: Timestamp) -> Vec<EngineEvent> {
        // Stops get parked; they don't match until triggered.
        if order.order_type.is_stop() {
            return self.add_stop_order(order, now);
        }

        let mut events = Vec::new();
        order.created_at = now;
        order.updated_at = now;
        if order.id == 0 {
            order.id = self.order_id_gen.next();
        }

        // ── Matching loop ──
        let mut trade_occurred = false;
        loop {
            if order.remaining() <= Quantity::ZERO {
                order.status = OrderStatus::Filled;
                break;
            }
            let best_price = match order.side {
                Side::Buy => self.book.asks.levels.keys().next().copied(),
                Side::Sell => self.book.bids.levels.keys().next_back().copied(),
            };
            let best_price = match best_price {
                Some(p) => p,
                None => break,
            };
            let crosses = match (order.side, order.price) {
                (Side::Buy, Some(limit)) => best_price <= limit,
                (Side::Sell, Some(limit)) => best_price >= limit,
                (Side::Buy, None) => true,
                (Side::Sell, None) => true,
            };
            if !crosses {
                break;
            }

            // Pop the oldest resting order at the best level.
            let (maker_id, level_now_empty) = match order.side {
                Side::Buy => {
                    let queue = self
                        .book
                        .asks
                        .levels
                        .get_mut(&best_price)
                        .expect("level must exist (we just read its key)");
                    let id = queue.pop_front().unwrap();
                    let empty = queue.is_empty();
                    (id, empty)
                }
                Side::Sell => {
                    let queue = self
                        .book
                        .bids
                        .levels
                        .get_mut(&best_price)
                        .expect("level must exist");
                    let id = queue.pop_front().unwrap();
                    let empty = queue.is_empty();
                    (id, empty)
                }
            };

            // Lookup the maker.
            let mut maker = match self.book.orders.remove(&maker_id) {
                Some(o) => o,
                None => continue,
            };

            // SMP: cancel the maker if same user. Level cleanup happens below.
            if maker.user == order.user {
                maker.status = OrderStatus::Cancelled;
                maker.updated_at = now;
                events.push(EngineEvent::Cancelled {
                    id: maker.id,
                    symbol: maker.symbol.clone(),
                    user: maker.user.clone(),
                    remaining: maker.remaining(),
                });
                if level_now_empty {
                    self.drop_level(order.side, best_price);
                    events.push(EngineEvent::BookDelta {
                        symbol: self.symbol.clone(),
                        side: order.side.opposite(),
                        price: best_price,
                        new_qty: Quantity::ZERO,
                        ts: now,
                    });
                }
                continue;
            }

            // Compute fill.
            let fill_qty = order.remaining().min(maker.remaining());
            let fill_price = best_price;

            // Mutate maker.
            maker.filled += fill_qty;
            maker.updated_at = now;
            if maker.filled >= maker.quantity {
                maker.status = OrderStatus::Filled;
                // Maker fully filled — drop the level if it had only this one
                // order.
                if level_now_empty {
                    self.drop_level(order.side, best_price);
                    events.push(EngineEvent::BookDelta {
                        symbol: self.symbol.clone(),
                        side: order.side.opposite(),
                        price: best_price,
                        new_qty: Quantity::ZERO,
                        ts: now,
                    });
                }
            } else {
                maker.status = OrderStatus::PartiallyFilled;
                // Put it back at the FRONT of its queue (same level, time priority).
                let side = match maker.side {
                    Side::Buy => &mut self.book.bids.levels,
                    Side::Sell => &mut self.book.asks.levels,
                };
                side.get_mut(&best_price)
                    .expect("level must still exist with partial fill")
                    .push_front(maker.id);
                self.book.orders.insert(maker.id, maker.clone());
                // Emit BookDelta for the partial-fill level with new total qty.
                let new_qty = self
                    .book
                    .asks
                    .level_qty(best_price, &self.book.orders)
                    .max(self.book.bids.level_qty(best_price, &self.book.orders));
                events.push(EngineEvent::BookDelta {
                    symbol: self.symbol.clone(),
                    side: order.side.opposite(),
                    price: best_price,
                    new_qty,
                    ts: now,
                });
            }

            // Mutate taker.
            order.filled += fill_qty;
            order.updated_at = now;
            order.status = if order.filled >= order.quantity {
                OrderStatus::Filled
            } else {
                OrderStatus::PartiallyFilled
            };

            // Emit Fill + Trade.
            let trade_id = self.trade_id_gen.next();
            let trade = Trade {
                id: trade_id,
                symbol: self.symbol.clone(),
                price: fill_price,
                quantity: fill_qty,
                buy_order_id: if order.side == Side::Buy {
                    order.id
                } else {
                    maker.id
                },
                sell_order_id: if order.side == Side::Sell {
                    order.id
                } else {
                    maker.id
                },
                taker_side: order.side,
                buyer: if order.side == Side::Buy {
                    order.user.clone()
                } else {
                    maker.user.clone()
                },
                seller: if order.side == Side::Sell {
                    order.user.clone()
                } else {
                    maker.user.clone()
                },
                timestamp: now,
                settle_status: SettleStatus::Pending,
            };
            events.push(EngineEvent::Fill {
                trade: trade.clone(),
                maker: maker.clone(),
                taker: order.clone(),
            });
            events.push(EngineEvent::Trade {
                trade: trade.clone(),
            });

            self.push_recent_trade(trade.clone());

            self.book.last_trade_price = Some(fill_price);
            let (candle, is_closed) = self.candle_aggregator.update(fill_price, fill_qty, now);
            events.push(EngineEvent::CandleUpdate {
                symbol: self.symbol.clone(),
                candle,
                is_closed,
            });
            trade_occurred = true;
        }

        // ── After matching ──
        match &order.order_type {
            OrderType::Limit | OrderType::StopLimit { .. } => {
                if order.remaining() > Quantity::ZERO && order.status != OrderStatus::Filled {
                    let price = order.price.expect("limit must have price");
                    events.push(EngineEvent::BookDelta {
                        symbol: self.symbol.clone(),
                        side: order.side,
                        price,
                        new_qty: order.remaining(),
                        ts: now,
                    });
                    self.book.insert(order.clone());
                }
            }
            OrderType::Market => {
                if order.filled == Quantity::ZERO {
                    order.status = OrderStatus::Cancelled;
                }
            }
            OrderType::Stop { .. } | OrderType::StopLimit { .. } => {
                unreachable!("stops should have been handled at the top");
            }
        }

        events.push(EngineEvent::Accepted {
            order: order.clone(),
        });

        if trade_occurred {
            if let Some(last) = self.book.last_trade_price {
                events.extend(self.trigger_stops(last, now));
            }
        }
        events
    }

    /// Drop an empty price level from the side's BTreeMap.
    fn drop_level(&mut self, side: Side, price: Price) {
        match side {
            Side::Buy => {
                self.book.asks.levels.remove(&price);
            }
            Side::Sell => {
                self.book.bids.levels.remove(&price);
            }
        }
    }

    // ── Stop orders ─────────────────────────────────────────

    fn add_stop_order(&mut self, mut order: Order, now: Timestamp) -> Vec<EngineEvent> {
        order.created_at = now;
        order.updated_at = now;
        if order.id == 0 {
            order.id = self.order_id_gen.next();
        }
        let trigger = match &order.order_type {
            OrderType::Stop { trigger } => *trigger,
            OrderType::StopLimit { trigger, .. } => *trigger,
            _ => unreachable!(),
        };
        self.stop_queue
            .entry(trigger)
            .or_default()
            .push_back(order.id);
        self.stop_orders.insert(order.id, order.clone());
        vec![EngineEvent::Accepted { order }]
    }

    /// Walk the stops BTreeMap and fire any that have been triggered by the
    /// given last trade price. Returns the events from each fired stop.
    pub fn trigger_stops(&mut self, last: Price, now: Timestamp) -> Vec<EngineEvent> {
        let mut events = Vec::new();
        let triggers: Vec<Price> = self
            .stop_queue
            .iter()
            .filter_map(|(trigger, queue)| {
                if queue.is_empty() {
                    return None;
                }
                let side = self
                    .stop_orders
                    .get(queue.front().unwrap())
                    .map(|o| o.side)
                    .unwrap_or(Side::Buy);
                match side {
                    Side::Buy if last >= *trigger => Some(*trigger),
                    Side::Sell if last <= *trigger => Some(*trigger),
                    _ => None,
                }
            })
            .collect();

        for trigger in triggers {
            let ids: Vec<OrderId> = self
                .stop_queue
                .get(&trigger)
                .map(|q| q.iter().copied().collect())
                .unwrap_or_default();
            for id in ids {
                self.stop_queue.get_mut(&trigger).map(|q| q.pop_front());
                if let Some(mut order) = self.stop_orders.remove(&id) {
                    // Convert Stop → Market, StopLimit → Limit. The Order's
                    // `price` field already holds the limit price for StopLimit,
                    // so we don't need to touch it.
                    order.order_type = match order.order_type {
                        OrderType::Stop { .. } => OrderType::Market,
                        OrderType::StopLimit { .. } => OrderType::Limit,
                        _ => unreachable!(),
                    };
                    if matches!(order.order_type, OrderType::Market) {
                        order.price = None;
                    }
                    events.extend(self.match_entry(order, now));
                }
            }
            if let Some(q) = self.stop_queue.get(&trigger) {
                if q.is_empty() {
                    self.stop_queue.remove(&trigger);
                }
            }
        }
        events
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TradeIdGen {
    next: TradeId,
}

impl TradeIdGen {
    pub fn new(start: TradeId) -> Self {
        Self { next: start }
    }
    pub fn next(&mut self) -> TradeId {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn limit(side: Side, price: Price, qty: Quantity) -> Order {
        limit_for("alice", side, price, qty)
    }

    fn limit_for(user: &str, side: Side, price: Price, qty: Quantity) -> Order {
        Order {
            id: 0,
            user: user.into(),
            symbol: "SOL-USDC".into(),
            side,
            order_type: OrderType::Limit,
            tif: TimeInForce::Gtc,
            price: Some(price),
            quantity: qty,
            filled: Quantity::ZERO,
            status: OrderStatus::New,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn limit_buy_rests_when_no_liquidity() {
        let mut eng = MatchingEngine::new("SOL-USDC".into());
        let order = limit(Side::Buy, dec!(100), dec!(1));
        let events = eng.match_entry(order, 1000);
        // Should produce at minimum one BookDelta and one Accepted.
        let book_delta_count = events
            .iter()
            .filter(|e| matches!(e, EngineEvent::BookDelta { .. }))
            .count();
        let accepted = events
            .iter()
            .filter(|e| matches!(e, EngineEvent::Accepted { .. }))
            .count();
        assert!(book_delta_count >= 1);
        assert_eq!(accepted, 1);
        assert_eq!(eng.open_order_count(), 1);
    }

    #[test]
    fn limit_buy_crosses_resting_sell_and_fills_at_resting_price() {
        let mut eng = MatchingEngine::new("SOL-USDC".into());
        // Resting sell at 100 qty 1.
        let resting = limit_for("alice", Side::Sell, dec!(100), dec!(1));
        let _ = eng.match_entry(resting, 1000);
        assert_eq!(eng.open_order_count(), 1);

        // Taker buy at 100 qty 1 → fully fills at resting price (100).
        let taker = limit_for("bob", Side::Buy, dec!(100), dec!(1));
        let events = eng.match_entry(taker, 2000);

        let fill_count = events
            .iter()
            .filter(|e| matches!(e, EngineEvent::Fill { .. }))
            .count();
        let trade_count = events
            .iter()
            .filter(|e| matches!(e, EngineEvent::Trade { .. }))
            .count();
        assert_eq!(fill_count, 1, "expected 1 fill");
        assert_eq!(trade_count, 1);

        // No orders left in the book (both fully filled).
        assert_eq!(eng.open_order_count(), 0);
        // Last trade price updated.
        assert_eq!(eng.book.last_trade_price, Some(dec!(100)));
    }

    #[test]
    fn partial_fill_keeps_maker_in_queue_at_same_level() {
        let mut eng = MatchingEngine::new("SOL-USDC".into());
        // Resting sell at 100 qty 5.
        let _ = eng.match_entry(limit_for("alice", Side::Sell, dec!(100), dec!(5)), 1000);

        // Taker buy at 100 qty 2 → fills 2 from maker; maker has 3 remaining.
        let events = eng.match_entry(limit_for("bob", Side::Buy, dec!(100), dec!(2)), 2000);
        let _ = events;

        assert_eq!(eng.open_order_count(), 1);
        let top = eng
            .book
            .asks
            .levels
            .get(&dec!(100))
            .expect("level should exist");
        assert_eq!(top.len(), 1);
    }

    #[test]
    fn price_time_priority_pop_front_on_match() {
        let mut eng = MatchingEngine::new("SOL-USDC".into());
        // Two resting sells at 100: alice first, bob second.
        let mut a = limit(Side::Sell, dec!(100), dec!(1));
        a.user = "alice".into();
        let mut b = limit(Side::Sell, dec!(100), dec!(1));
        b.user = "bob".into();
        let _ = eng.match_entry(a, 1000);
        let _ = eng.match_entry(b, 1000);

        // Capture alice's id BEFORE the fill removes her from the orders map.
        let alice_id = eng
            .book
            .orders
            .iter()
            .find(|(_, o)| o.user == "alice")
            .map(|(id, _)| *id)
            .expect("alice should be in the book");
        let bob_id = eng
            .book
            .orders
            .iter()
            .find(|(_, o)| o.user == "bob")
            .map(|(id, _)| *id)
            .expect("bob should be in the book");

        // Taker buys qty 1 → matches alice first (FIFO).
        let mut t = limit(Side::Buy, dec!(100), dec!(1));
        t.user = "carol".into();
        let events = eng.match_entry(t, 2000);

        let fill = events
            .iter()
            .find_map(|e| match e {
                EngineEvent::Fill { trade, .. } => Some(trade),
                _ => None,
            })
            .expect("fill event");

        assert_eq!(
            fill.sell_order_id, alice_id,
            "FIFO: should match alice first"
        );
        assert_ne!(
            fill.sell_order_id, bob_id,
            "FIFO: should not match bob first"
        );
        // Bob still resting.
        assert!(eng.book.orders.contains_key(&bob_id));
        assert!(!eng.book.orders.contains_key(&alice_id));
    }

    #[test]
    fn smp_cancels_maker() {
        let mut eng = MatchingEngine::new("SOL-USDC".into());
        // alice places sell.
        let mut sell = limit(Side::Sell, dec!(100), dec!(1));
        sell.user = "alice".into();
        let _ = eng.match_entry(sell, 1000);

        // alice tries to buy at the same price → SMP cancel-maker.
        let mut buy = limit(Side::Buy, dec!(100), dec!(1));
        buy.user = "alice".into();
        let events = eng.match_entry(buy, 2000);

        let cancel_count = events
            .iter()
            .filter(|e| matches!(e, EngineEvent::Cancelled { .. }))
            .count();
        // Cancelled event for alice's sell + Accepted for her buy (which now rests).
        assert!(cancel_count >= 1, "SMP should cancel the maker");
    }

    #[test]
    fn cancel_removes_resting_order() {
        let mut eng = MatchingEngine::new("SOL-USDC".into());
        let _ = eng.match_entry(limit(Side::Buy, dec!(100), dec!(1)), 1000);
        assert_eq!(eng.open_order_count(), 1);

        let oid = *eng.book.orders.keys().next().unwrap();
        let events = eng.cancel(oid, 2000);
        assert_eq!(eng.open_order_count(), 0);
        let cancelled = events
            .iter()
            .any(|e| matches!(e, EngineEvent::Cancelled { .. }));
        assert!(cancelled);
    }
}
