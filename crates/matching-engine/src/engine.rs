//! The matching engine.
//!
//! Strict price-time priority. Two `BTreeMap<Price, VecDeque<Order>>` —
//! one per side — give O(log n) best-price lookup and O(1) FIFO at each
//! level. A `HashMap<OrderId, OrderLocation>` provides O(1) cancellation
//! lookup; cancellation within a level is O(n) in the level size, but
//! levels are typically small.
//!
//! ## Market order behavior on partial fill
//!
//! A market order that cannot be fully filled against available liquidity
//! has its unfilled remainder **silently cancelled**. This matches
//! Binance/Coinbase/Kraken behavior. The caller learns the size of the
//! cancelled remainder via `MatchResult::cancelled_remainder_qty`. Limit
//! orders, in contrast, always rest any unfilled remainder on the book.
//!
//! ## Order id uniqueness
//!
//! In real exchanges order ids are globally unique per session; the engine
//! enforces the same invariant via `EngineError::DuplicateOrderId`. After
//! a cancel or full fill the id becomes available for reuse.

use std::collections::{BTreeMap, HashMap, VecDeque};

use common::{Order, OrderId, OrderKind, Price, Qty, Side, Timestamp};

use crate::types::{EngineError, MatchResult, Snapshot, Trade};

/// Internal index entry: which side and price a resting order sits at,
/// for O(1) cancel lookup of the (side, price) pair. Position within the
/// level is found by linear scan (levels are typically small).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OrderLocation {
    side: Side,
    price: Price,
}

/// The matching engine for a single symbol.
///
/// In Phase 2, one `MatchingEngine` instance will live behind a dedicated
/// actor task per symbol, receiving commands over a channel.
#[derive(Debug, Default)]
pub struct MatchingEngine {
    /// Bids keyed by price ascending; best bid is `keys().next_back()`.
    bids: BTreeMap<Price, VecDeque<Order>>,
    /// Asks keyed by price ascending; best ask is `keys().next()`.
    asks: BTreeMap<Price, VecDeque<Order>>,
    /// O(1) cancel lookup.
    index: HashMap<OrderId, OrderLocation>,
    /// Monotonic timestamp counter; injected into orders on construction
    /// if the caller leaves it at 0. Lets tests and benches get unique
    /// timestamps without a clock. NOT used to override caller-provided
    /// timestamps — only fills in 0.
    next_ts: u64,
}

impl MatchingEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Submit a limit order. Matches against the opposite side as long as
    /// the limit price crosses; rests any remainder on the book.
    pub fn submit_limit(&mut self, mut order: Order) -> Result<MatchResult, EngineError> {
        if order.qty.is_zero() {
            return Err(EngineError::ZeroQuantity);
        }
        if order.price.is_none() {
            return Err(EngineError::LimitOrderWithoutPrice);
        }
        if order.kind != OrderKind::Limit {
            return Err(EngineError::MarketOrderWithPrice);
        }

        self.fill_timestamp(&mut order);
        let result = self.match_taker(order)?;
        Ok(result)
    }

    /// Submit a market order. Sweeps the opposite side. Any unfilled
    /// remainder is cancelled.
    pub fn submit_market(&mut self, mut order: Order) -> Result<MatchResult, EngineError> {
        if order.qty.is_zero() {
            return Err(EngineError::ZeroQuantity);
        }
        if order.price.is_some() {
            return Err(EngineError::MarketOrderWithPrice);
        }
        if order.kind != OrderKind::Market {
            return Err(EngineError::LimitOrderWithoutPrice);
        }

        self.fill_timestamp(&mut order);
        let result = self.match_taker(order)?;
        Ok(result)
    }

    /// Cancel a resting order by id. Returns `UnknownOrder` if the id is
    /// not currently resting (never submitted, already cancelled, or
    /// already fully filled).
    pub fn cancel(&mut self, id: OrderId) -> Result<(), EngineError> {
        let loc = self.index.remove(&id).ok_or(EngineError::UnknownOrder)?;
        let level = match loc.side {
            Side::Buy => self.bids.get_mut(&loc.price),
            Side::Sell => self.asks.get_mut(&loc.price),
        };
        let level = level.ok_or(EngineError::UnknownOrder)?;

        // Linear scan within the level. Levels are typically small.
        let pos = level
            .iter()
            .position(|o| o.id == id)
            .ok_or(EngineError::UnknownOrder)?;
        level.remove(pos);

        if level.is_empty() {
            match loc.side {
                Side::Buy => {
                    self.bids.remove(&loc.price);
                }
                Side::Sell => {
                    self.asks.remove(&loc.price);
                }
            }
        }
        Ok(())
    }

    /// Best (highest) bid price, or `None` if no bids.
    pub fn best_bid(&self) -> Option<Price> {
        self.bids.keys().next_back().copied()
    }

    /// Best (lowest) ask price, or `None` if no asks.
    pub fn best_ask(&self) -> Option<Price> {
        self.asks.keys().next().copied()
    }

    /// Aggregated snapshot. Bids descending, asks ascending.
    pub fn snapshot(&self) -> Snapshot {
        let mut bids: Vec<(Price, Qty)> = self
            .bids
            .iter()
            .map(|(p, q)| {
                let total = q.iter().fold(0u64, |acc, o| acc + o.qty.0);
                (*p, Qty(total))
            })
            .collect();
        bids.reverse(); // descending

        let asks: Vec<(Price, Qty)> = self
            .asks
            .iter()
            .map(|(p, q)| {
                let total = q.iter().fold(0u64, |acc, o| acc + o.qty.0);
                (*p, Qty(total))
            })
            .collect();

        Snapshot { bids, asks }
    }

    /// Total resting quantity across both sides. Used by invariant tests.
    pub fn total_resting_qty(&self) -> Qty {
        let total = self
            .bids
            .values()
            .chain(self.asks.values())
            .flat_map(|dq| dq.iter())
            .fold(0u64, |acc, o| acc + o.qty.0);
        Qty(total)
    }

    /// Number of resting orders (across both sides).
    pub fn resting_order_count(&self) -> usize {
        let bids: usize = self.bids.values().map(|dq| dq.len()).sum();
        let asks: usize = self.asks.values().map(|dq| dq.len()).sum();
        bids + asks
    }

    // ----------------- internal -----------------

    /// If the caller left `order.timestamp` at 0, fill in a monotonic value.
    /// Real callers (the API layer in Phase 5) will inject timestamps.
    fn fill_timestamp(&mut self, order: &mut Order) {
        if order.timestamp == Timestamp(0) {
            self.next_ts += 1;
            order.timestamp = Timestamp(self.next_ts);
        }
    }

    /// Run the matching loop for a taker. Returns the trades and decides
    /// what to do with any remainder (rest for limit, cancel for market).
    ///
    /// Returns `DuplicateOrderId` if the taker's id is already resting —
    /// we check this up-front so the book is never partially mutated.
    fn match_taker(&mut self, mut taker: Order) -> Result<MatchResult, EngineError> {
        if self.index.contains_key(&taker.id) {
            return Err(EngineError::DuplicateOrderId);
        }

        let mut trades = Vec::new();
        let mut taker_remaining = taker.qty;
        let taker_id = taker.id;

        loop {
            if taker_remaining.is_zero() {
                break;
            }

            // Best price on the opposite side (the one we'd match against).
            let opposite_price = match taker.side {
                Side::Buy => self.best_ask(),
                Side::Sell => self.best_bid(),
            };
            let Some(opposite_price) = opposite_price else {
                break;
            };

            // Limit orders only match if their price crosses the opposite.
            if !taker_crosses(&taker, opposite_price) {
                break;
            }

            // Capture maker id BEFORE popping, so we can record it in the trade.
            // Also track whether the *level* is now empty so we can drop
            // it from the book before the next iteration.
            let (maker_id, fill_qty, level_now_empty) = {
                let level = match taker.side {
                    Side::Buy => self
                        .asks
                        .get_mut(&opposite_price)
                        .expect("best_ask just returned this price"),
                    Side::Sell => self
                        .bids
                        .get_mut(&opposite_price)
                        .expect("best_bid just returned this price"),
                };
                let maker = level.pop_front().expect("level non-empty");
                let maker_id = maker.id;
                let fill = if taker_remaining.0 < maker.qty.0 {
                    taker_remaining
                } else {
                    maker.qty
                };
                let maker_after = maker.qty - fill;
                if !maker_after.is_zero() {
                    level.push_front(maker.with_qty(maker_after));
                } else {
                    // Maker fully consumed → remove from index. If this
                    // was the last maker at the level, the level is now
                    // empty and we'll drop the whole price entry below.
                    self.index.remove(&maker_id);
                }
                (maker_id, fill, level.is_empty())
            };

            // If the level is now empty (popped the last maker at that
            // price), drop the price entry from the book so the next
            // iteration's best_* doesn't return a phantom price.
            if level_now_empty {
                match taker.side {
                    Side::Buy => {
                        self.asks.remove(&opposite_price);
                    }
                    Side::Sell => {
                        self.bids.remove(&opposite_price);
                    }
                }
            }

            trades.push(Trade {
                maker_order_id: maker_id,
                taker_order_id: taker_id,
                price: opposite_price,
                qty: fill_qty,
            });

            taker_remaining = taker_remaining - fill_qty;
        }

        // Drop any levels that became empty. (The matching loop above
        // already removes levels when their last maker is fully consumed;
        // this is belt-and-braces in case a future code path leaves an
        // empty level behind.)
        self.bids.retain(|_, dq| !dq.is_empty());
        self.asks.retain(|_, dq| !dq.is_empty());

        // Decide taker's fate.
        let cancelled_remainder_qty = if taker.kind == OrderKind::Market {
            taker_remaining
        } else {
            Qty::ZERO
        };

        let resting_order_id = if taker.kind == OrderKind::Limit && !taker_remaining.is_zero() {
            taker.qty = taker_remaining;
            self.insert_resting(taker);
            Some(taker_id)
        } else {
            None
        };

        Ok(MatchResult {
            trades,
            resting_order_id,
            cancelled_remainder_qty,
        })
    }

    /// Place a limit taker's remainder on the book. Caller must have
    /// verified the id is not already resting.
    fn insert_resting(&mut self, order: Order) {
        let price = order.price.expect("limit order has a price");
        let id = order.id;
        let side = order.side;
        let level = match side {
            Side::Buy => self.bids.entry(price).or_default(),
            Side::Sell => self.asks.entry(price).or_default(),
        };
        level.push_back(order);
        self.index.insert(id, OrderLocation { side, price });
    }
}

/// True if `taker` would match at `opposite_price`. Limit orders require
/// their price to cross; market orders always do.
fn taker_crosses(taker: &Order, opposite_price: Price) -> bool {
    match (taker.kind, taker.price) {
        (OrderKind::Market, _) => true,
        (OrderKind::Limit, Some(p)) => match taker.side {
            Side::Buy => p >= opposite_price,
            Side::Sell => p <= opposite_price,
        },
        (OrderKind::Limit, None) => false,
    }
}

/// Tiny helper: clone an Order with a different qty without exposing
/// mutable access to fields from outside the crate.
trait OrderExt: Sized {
    fn with_qty(self, qty: Qty) -> Self;
}

impl OrderExt for Order {
    fn with_qty(mut self, qty: Qty) -> Self {
        self.qty = qty;
        self
    }
}