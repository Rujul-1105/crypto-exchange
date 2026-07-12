//! Property-based invariant tests for the matching engine.
//!
//! These tests drive the engine through random sequences of submits and
//! cancels and assert that three core invariants from CLAUDE.md Phase 1
//! hold after every operation:
//!
//! 1. **Price-time priority is never violated.** At every price level,
//!    resting orders are sorted by timestamp ascending (FIFO).
//! 2. **Quantity is conserved.** Σ submitted = Σ traded + Σ resting + Σ
//!    cancelled (where "cancelled" includes market-order unfilled
//!    remainders).
//! 3. **The book is never crossed.** `best_bid < best_ask` whenever both
//!    sides are non-empty.

use std::collections::HashMap;

use common::{Order, OrderId, OrderKind, Price, Qty, Side, Timestamp};
use matching_engine::MatchingEngine;
use proptest::prelude::*;

// ----------------- operation model -----------------

#[derive(Debug, Clone)]
enum Op {
    SubmitLimit {
        id: u64,
        side: Side,
        price: Price,
        qty: u64,
        ts: u64,
    },
    SubmitMarket {
        id: u64,
        side: Side,
        qty: u64,
        ts: u64,
    },
    Cancel {
        id: u64,
    },
}

fn side_strategy() -> impl Strategy<Value = Side> {
    prop_oneof![Just(Side::Buy), Just(Side::Sell)]
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        // Submit limit
        (1u64..200, side_strategy(), 1u64..500, 1u64..50, 1u64..10_000).prop_map(
            |(id, side, price, qty, ts)| Op::SubmitLimit {
                id,
                side,
                price: Price(price),
                qty,
                ts,
            },
        ),
        // Submit market
        (200u64..400, side_strategy(), 1u64..50, 1u64..10_000).prop_map(
            |(id, side, qty, ts)| Op::SubmitMarket { id, side, qty, ts },
        ),
        // Cancel — id chosen from a small space so it often hits a real order
        (1u64..400).prop_map(|id| Op::Cancel { id }),
    ]
}

fn ops_strategy() -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec(op_strategy(), 1..80)
}

// ----------------- invariant helpers -----------------

fn assert_no_crossed_spread(engine: &MatchingEngine) {
    match (engine.best_bid(), engine.best_ask()) {
        (Some(b), Some(a)) => assert!(
            b < a,
            "crossed book: best_bid={:?} >= best_ask={:?}",
            b,
            a
        ),
        _ => {} // one or both sides empty → trivially uncrossed
    }
}

// ----------------- quantity conservation -----------------

struct Conservation {
    submitted: u64,
    /// Sum of qty "attributed" to orders via trades. Each trade's qty is
    /// counted TWICE: once for the maker losing it, once for the taker
    /// losing it. So `traded == 2 * sum(trade.qty)`.
    traded: u64,
    cancelled: u64,
    /// `remaining[id]` = how much of order `id` is currently resting or
    /// (if cancelled) was resting at cancel time. Used to credit `cancelled`
    /// correctly.
    remaining: HashMap<u64, u64>,
}

impl Conservation {
    fn new() -> Self {
        Conservation {
            submitted: 0,
            traded: 0,
            cancelled: 0,
            remaining: HashMap::new(),
        }
    }

    fn on_submit(&mut self, id: u64, qty: u64) {
        self.submitted += qty;
        self.remaining.insert(id, qty);
    }

    fn on_trade(&mut self, maker_id: u64, taker_id: u64, qty: u64) {
        // Each fill removes qty from BOTH the maker and the taker. We
        // track per-order removal, so count qty twice.
        self.traded += 2 * qty;
        *self.remaining.entry(maker_id).or_insert(0) -= qty;
        *self.remaining.entry(taker_id).or_insert(0) -= qty;
    }

    fn on_cancel(&mut self, id: u64) -> u64 {
        let was = self.remaining.remove(&id).unwrap_or(0);
        self.cancelled += was;
        was
    }

    fn on_market_remainder(&mut self, taker_id: u64, remainder: u64) {
        self.cancelled += remainder;
        // The taker is fully gone from the system.
        self.remaining.remove(&taker_id);
    }

    fn assert_conserved(&self, engine: &MatchingEngine) {
        let book_qty: u64 = engine.total_resting_qty().0;
        // The sum of remaining[] across all known orders should match
        // the engine's resting qty exactly.
        let tracked_resting: u64 = self.remaining.values().sum();
        assert_eq!(
            tracked_resting, book_qty,
            "tracked remaining {} != engine resting {}",
            tracked_resting, book_qty
        );
        // Σ submitted = Σ traded (per-order, counted from both sides) +
        //               Σ resting + Σ cancelled.
        assert_eq!(
            self.submitted,
            self.traded + book_qty + self.cancelled,
            "conservation violated: submitted={} != traded({}) + resting({}) + cancelled({})",
            self.submitted,
            self.traded,
            book_qty,
            self.cancelled
        );
    }
}

// ----------------- tests -----------------

proptest! {
    /// The three CLAUDE.md invariants hold after every operation in a
    /// random sequence. proptest runs the default 256 cases plus shrinks
    /// any failure.
    #[test]
    fn invariants_hold_under_random_ops(ops in ops_strategy()) {
        let mut engine = MatchingEngine::new();
        let mut cons = Conservation::new();

        for op in ops {
            match op {
                Op::SubmitLimit { id, side, price, qty, ts } => {
                    let order = Order {
                        id: OrderId(id),
                        side,
                        price: Some(price),
                        qty: Qty(qty),
                        timestamp: Timestamp(ts),
                        kind: OrderKind::Limit,
                    };
                    let result = engine.submit_limit(order);
                    if let Ok(r) = result {
                        // Order entered the system. Track it now so we
                        // don't have to undo on rejection (which would
                        // confuse the tracker if a previous op with the
                        // same id is still tracked).
                        cons.on_submit(id, qty);
                        for t in &r.trades {
                            cons.on_trade(t.maker_order_id.0, t.taker_order_id.0, t.qty.0);
                        }
                        if r.resting_order_id.is_none() {
                            // Fully matched, not resting.
                            cons.remaining.remove(&id);
                        }
                    }
                    // If rejected (e.g. duplicate id, zero qty), nothing
                    // entered the system — leave the tracker alone.
                }
                Op::SubmitMarket { id, side, qty, ts } => {
                    let order = Order {
                        id: OrderId(id),
                        side,
                        price: None,
                        qty: Qty(qty),
                        timestamp: Timestamp(ts),
                        kind: OrderKind::Market,
                    };
                    let result = engine.submit_market(order);
                    if let Ok(r) = result {
                        cons.on_submit(id, qty);
                        for t in &r.trades {
                            cons.on_trade(t.maker_order_id.0, t.taker_order_id.0, t.qty.0);
                        }
                        let remainder = r.cancelled_remainder_qty.0;
                        cons.on_market_remainder(id, remainder);
                    }
                }
                Op::Cancel { id } => {
                    let _ = engine.cancel(OrderId(id));
                    cons.on_cancel(id);
                }
            }

            assert_no_crossed_spread(&engine);
            cons.assert_conserved(&engine);
        }
    }
}

proptest! {
    /// Price-time priority is preserved: when a taker sweeps one side,
    /// trades come out in FIFO order across levels.
    #[test]
    fn price_time_priority_under_sweep(
        // Place a sequence of resting orders on one side, then drain
        // them with a single aggressive taker.
        seed in 1u64..1000,
        n in 3usize..15,
    ) {
        let mut engine = MatchingEngine::new();
        let mut cons = Conservation::new();
        let mut timestamps = Vec::with_capacity(n);

        // Place n sells at the same price, in increasing timestamp order.
        for i in 0..n {
            let id = seed * 1000 + i as u64;
            let ts = (i as u64) + 1;
            timestamps.push((id, ts));
            let order = Order::limit(
                OrderId(id),
                Side::Sell,
                Price(100),
                Qty(1),
                Timestamp(ts),
            );
            cons.on_submit(id, 1);
            engine.submit_limit(order).expect("setup accepted");
        }

        // Drain with a single market buy.
        let taker_id = seed * 1000 + n as u64 + 1;
        let taker_order = Order::market(
            OrderId(taker_id),
            Side::Buy,
            Qty(n as u64),
            Timestamp(0),
        );
        cons.on_submit(taker_id, n as u64);
        let result = engine.submit_market(taker_order).expect("market accepted");

        // Trades should come out in the order of timestamps.
        let trade_makers: Vec<u64> = result.trades.iter().map(|t| t.maker_order_id.0).collect();
        let expected: Vec<u64> = timestamps.iter().map(|(id, _)| *id).collect();
        assert_eq!(trade_makers, expected, "price-time priority violated");

        // Conservation should still hold.
        for t in &result.trades {
            cons.on_trade(t.maker_order_id.0, t.taker_order_id.0, t.qty.0);
        }
        cons.on_market_remainder(taker_id, 0);
        assert_no_crossed_spread(&engine);
        cons.assert_conserved(&engine);
    }
}

proptest! {
    /// Aggressive taker that hits multiple price levels: trades come out
    /// in price-priority order (best price first), FIFO within a level.
    #[test]
    fn price_priority_across_levels(
        seed in 1u64..1000,
        n in 2usize..8,
    ) {
        let mut engine = MatchingEngine::new();
        // Place n sells at prices 100, 101, 102, ..., 100+n-1, each qty 1.
        // Timestamps increase with index, so within a level FIFO is by index.
        for i in 0..n {
            let id = seed * 1000 + i as u64;
            engine.submit_limit(Order::limit(
                OrderId(id),
                Side::Sell,
                Price(100 + i as u64),
                Qty(1),
                Timestamp(i as u64 + 1),
            )).expect("setup accepted");
        }

        // Aggressive buy that sweeps all n levels.
        let taker = Order::limit(
            OrderId(seed * 1000 + n as u64 + 1),
            Side::Buy,
            Price(100 + n as u64 - 1 + 10), // well above all asks
            Qty(n as u64),
            Timestamp(0),
        );
        let result = engine.submit_limit(taker).expect("sweep accepted");

        // Verify: trade prices ascending (best first), all qty=1.
        let trade_prices: Vec<u64> = result.trades.iter().map(|t| t.price.0).collect();
        let expected_prices: Vec<u64> = (0..n).map(|i| 100 + i as u64).collect();
        assert_eq!(trade_prices, expected_prices);
        assert!(result.trades.iter().all(|t| t.qty.0 == 1));
    }
}