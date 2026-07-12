//! Edge-case unit tests for the matching engine.
//!
//! These complement the property-based invariant tests in `invariants.rs`
//! by exercising specific scenarios in a deterministic, named form.
//! Every test here corresponds to one bullet in CLAUDE.md Phase 1's edge
//! case list.

use common::{Order, OrderId, Price, Qty, Side, Timestamp};
use matching_engine::{EngineError, MatchingEngine, Trade};

/// Tiny helper to construct a limit order with a sequential id.
fn lim(id: u64, side: Side, price: Price, qty: u64) -> Order {
    Order::limit(
        OrderId(id),
        side,
        price,
        Qty(qty),
        Timestamp(id),
    )
}

fn mkt(id: u64, side: Side, qty: u64) -> Order {
    Order::market(OrderId(id), side, Qty(qty), Timestamp(id))
}

/// Build a Trade for assertions without going through `From` (orphan rule).
fn trade(maker: u64, taker: u64, price: u64, qty: u64) -> Trade {
    Trade {
        maker_order_id: OrderId(maker),
        taker_order_id: OrderId(taker),
        price: Price(price),
        qty: Qty(qty),
    }
}

// ----------------- zero-qty rejection -----------------

#[test]
fn submit_limit_with_zero_qty_is_rejected() {
    let mut engine = MatchingEngine::new();
    let result = engine.submit_limit(Order::limit(
        OrderId(1),
        Side::Buy,
        Price(100),
        Qty(0),
        Timestamp(1),
    ));
    assert_eq!(result, Err(EngineError::ZeroQuantity));
    assert_eq!(engine.resting_order_count(), 0);
}

#[test]
fn submit_market_with_zero_qty_is_rejected() {
    let mut engine = MatchingEngine::new();
    let result = engine.submit_market(Order::market(
        OrderId(1),
        Side::Buy,
        Qty(0),
        Timestamp(1),
    ));
    assert_eq!(result, Err(EngineError::ZeroQuantity));
}

// ----------------- limit/market mismatch -----------------

#[test]
fn limit_order_without_price_is_rejected() {
    let mut engine = MatchingEngine::new();
    let mut order = Order::limit(OrderId(1), Side::Buy, Price(100), Qty(5), Timestamp(1));
    order.price = None; // caller violates the invariant
    let result = engine.submit_limit(order);
    assert_eq!(result, Err(EngineError::LimitOrderWithoutPrice));
}

#[test]
fn market_order_with_price_is_rejected() {
    let mut engine = MatchingEngine::new();
    let mut order = Order::market(OrderId(1), Side::Buy, Qty(5), Timestamp(1));
    order.price = Some(Price(100)); // caller violates the invariant
    let result = engine.submit_market(order);
    assert_eq!(result, Err(EngineError::MarketOrderWithPrice));
}

// ----------------- self-crossing (aggressive orders) -----------------

#[test]
fn aggressive_buy_sweeps_multiple_ask_levels_at_resting_prices() {
    let mut engine = MatchingEngine::new();
    // Rest three asks at 100, 101, 102.
    engine.submit_limit(lim(1, Side::Sell, Price(100), 5)).unwrap();
    engine.submit_limit(lim(2, Side::Sell, Price(101), 5)).unwrap();
    engine.submit_limit(lim(3, Side::Sell, Price(102), 5)).unwrap();

    // Buy 12 @ 105 — fills 5@100, 5@101, 2@102. qty 12 is exactly consumed
    // (5+5+2), so nothing rests.
    let result = engine.submit_limit(lim(4, Side::Buy, Price(105), 12)).unwrap();

    assert_eq!(result.trades, vec![
        trade(1, 4, 100, 5),
        trade(2, 4, 101, 5),
        trade(3, 4, 102, 2),
    ]);
    assert_eq!(result.resting_order_id, None);
    assert_eq!(result.cancelled_remainder_qty, Qty::ZERO);
    // After consuming 2 from seller 3, that order still has 3 units
    // resting at 102.
    assert_eq!(engine.best_ask(), Some(Price(102)));
    assert_eq!(engine.snapshot().asks, vec![(Price(102), Qty(3))]);
}

#[test]
fn aggressive_buy_with_overshoot_rests_remainder() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Sell, Price(100), 5)).unwrap();
    engine.submit_limit(lim(2, Side::Sell, Price(101), 5)).unwrap();
    engine.submit_limit(lim(3, Side::Sell, Price(102), 5)).unwrap();

    // Buy 22 @ 105 — fills 5+5+5=15, then 7 rests at 105.
    let result = engine.submit_limit(lim(4, Side::Buy, Price(105), 22)).unwrap();

    assert_eq!(result.trades, vec![
        trade(1, 4, 100, 5),
        trade(2, 4, 101, 5),
        trade(3, 4, 102, 5),
    ]);
    assert_eq!(result.resting_order_id, Some(OrderId(4)));
    assert_eq!(engine.best_bid(), Some(Price(105)));
    assert_eq!(engine.best_ask(), None);
    assert_eq!(engine.resting_order_count(), 1);
}

#[test]
fn aggressive_sell_sweeps_multiple_bid_levels_at_resting_prices() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Buy, Price(105), 5)).unwrap();
    engine.submit_limit(lim(2, Side::Buy, Price(104), 5)).unwrap();
    engine.submit_limit(lim(3, Side::Buy, Price(103), 5)).unwrap();

    let result = engine.submit_limit(lim(4, Side::Sell, Price(100), 12)).unwrap();

    assert_eq!(result.trades, vec![
        trade(1, 4, 105, 5),
        trade(2, 4, 104, 5),
        trade(3, 4, 103, 2),
    ]);
    assert_eq!(result.resting_order_id, None);
    // After consuming 2 from buyer 3, that order still has 3 units
    // resting at 103.
    assert_eq!(engine.best_bid(), Some(Price(103)));
    assert_eq!(engine.snapshot().bids, vec![(Price(103), Qty(3))]);
}

// ----------------- cancel-after-partial-fill -----------------

#[test]
fn cancel_after_partial_fill_removes_remainder_from_book() {
    let mut engine = MatchingEngine::new();
    // Maker rests with 100 units.
    engine.submit_limit(lim(1, Side::Sell, Price(100), 100)).unwrap();
    // Taker fills 30, leaving 70 on the maker's order.
    let partial = engine.submit_limit(lim(2, Side::Buy, Price(100), 30)).unwrap();
    assert_eq!(partial.trades, vec![trade(1, 2, 100, 30)]);

    // Book still has the maker with 70 remaining.
    assert_eq!(engine.best_ask(), Some(Price(100)));
    let snap = engine.snapshot();
    assert_eq!(snap.asks, vec![(Price(100), Qty(70))]);

    // Cancel the maker; remainder should be removed.
    engine.cancel(OrderId(1)).unwrap();

    // Book is empty on ask side; cancel again returns UnknownOrder.
    assert_eq!(engine.best_ask(), None);
    assert_eq!(engine.cancel(OrderId(1)), Err(EngineError::UnknownOrder));
}

#[test]
fn cancel_unknown_order_is_rejected() {
    let mut engine = MatchingEngine::new();
    assert_eq!(
        engine.cancel(OrderId(999)),
        Err(EngineError::UnknownOrder)
    );
}

#[test]
fn double_cancel_is_rejected() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Buy, Price(100), 5)).unwrap();
    engine.cancel(OrderId(1)).unwrap();
    assert_eq!(engine.cancel(OrderId(1)), Err(EngineError::UnknownOrder));
}

#[test]
fn duplicate_order_id_is_rejected() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Buy, Price(100), 5)).unwrap();
    // Second submit with the same id must be rejected without mutating
    // the book.
    let result = engine.submit_limit(lim(1, Side::Buy, Price(101), 5));
    assert_eq!(result, Err(EngineError::DuplicateOrderId));
    assert_eq!(engine.resting_order_count(), 1);
}

#[test]
fn id_can_be_reused_after_cancel() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Buy, Price(100), 5)).unwrap();
    engine.cancel(OrderId(1)).unwrap();
    // After cancel the id is free again.
    engine.submit_limit(lim(1, Side::Sell, Price(101), 5)).unwrap();
    assert_eq!(engine.resting_order_count(), 1);
}

#[test]
fn id_can_be_reused_after_full_fill() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Sell, Price(100), 5)).unwrap();
    // Buy fully consumes order 1.
    engine.submit_limit(lim(2, Side::Buy, Price(100), 5)).unwrap();
    // After full fill, id 1 is free.
    engine.submit_limit(lim(1, Side::Sell, Price(101), 5)).unwrap();
    assert_eq!(engine.resting_order_count(), 1);
}

// ----------------- fully-matched / fully-rested / no-match -----------------

#[test]
fn fully_matched_limit_emits_trade_and_no_resting() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Sell, Price(100), 5)).unwrap();
    let result = engine.submit_limit(lim(2, Side::Buy, Price(100), 5)).unwrap();
    assert_eq!(result.trades, vec![trade(1, 2, 100, 5)]);
    assert_eq!(result.resting_order_id, None);
    assert_eq!(result.cancelled_remainder_qty, Qty::ZERO);
    assert_eq!(engine.resting_order_count(), 0);
}

#[test]
fn fully_rested_limit_emits_no_trades_and_rests() {
    let mut engine = MatchingEngine::new();
    let result = engine.submit_limit(lim(1, Side::Buy, Price(100), 5)).unwrap();
    assert!(result.trades.is_empty());
    assert_eq!(result.resting_order_id, Some(OrderId(1)));
    assert_eq!(result.cancelled_remainder_qty, Qty::ZERO);
    assert_eq!(engine.best_bid(), Some(Price(100)));
    assert_eq!(engine.best_ask(), None);
}

#[test]
fn market_order_on_empty_book_is_cancelled() {
    let mut engine = MatchingEngine::new();
    let result = engine.submit_market(mkt(1, Side::Buy, 50)).unwrap();
    assert!(result.trades.is_empty());
    assert_eq!(result.resting_order_id, None);
    assert_eq!(result.cancelled_remainder_qty, Qty(50));
}

#[test]
fn market_order_partial_fill_reports_cancelled_remainder() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Sell, Price(100), 30)).unwrap();
    let result = engine.submit_market(mkt(2, Side::Buy, 50)).unwrap();
    assert_eq!(result.trades, vec![trade(1, 2, 100, 30)]);
    assert_eq!(result.cancelled_remainder_qty, Qty(20));
    assert_eq!(engine.best_ask(), None);
}

// ----------------- price-time priority (deterministic) -----------------

#[test]
fn at_same_price_level_oldest_order_fills_first() {
    let mut engine = MatchingEngine::new();
    // Three sellers at the same price, different timestamps.
    engine
        .submit_limit(Order::limit(OrderId(1), Side::Sell, Price(100), Qty(3), Timestamp(1)))
        .unwrap();
    engine
        .submit_limit(Order::limit(OrderId(2), Side::Sell, Price(100), Qty(3), Timestamp(2)))
        .unwrap();
    engine
        .submit_limit(Order::limit(OrderId(3), Side::Sell, Price(100), Qty(3), Timestamp(3)))
        .unwrap();

    // A buy of 5 should hit order 1 first, then order 2.
    let result = engine.submit_limit(lim(4, Side::Buy, Price(100), 5)).unwrap();
    assert_eq!(
        result.trades,
        vec![trade(1, 4, 100, 3), trade(2, 4, 100, 2)]
    );
}

// ----------------- best bid / ask -----------------

#[test]
fn best_bid_and_ask_pick_extreme_prices() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Buy, Price(99), 1)).unwrap();
    engine.submit_limit(lim(2, Side::Buy, Price(101), 1)).unwrap();
    engine.submit_limit(lim(3, Side::Buy, Price(100), 1)).unwrap();
    engine.submit_limit(lim(4, Side::Sell, Price(105), 1)).unwrap();
    engine.submit_limit(lim(5, Side::Sell, Price(103), 1)).unwrap();
    engine.submit_limit(lim(6, Side::Sell, Price(104), 1)).unwrap();

    assert_eq!(engine.best_bid(), Some(Price(101)));
    assert_eq!(engine.best_ask(), Some(Price(103)));
    assert!(engine.best_bid().unwrap() < engine.best_ask().unwrap());
}

// ----------------- snapshot aggregation -----------------

#[test]
fn snapshot_aggregates_qty_per_price_level() {
    let mut engine = MatchingEngine::new();
    engine.submit_limit(lim(1, Side::Buy, Price(100), 5)).unwrap();
    engine.submit_limit(lim(2, Side::Buy, Price(100), 7)).unwrap();
    engine.submit_limit(lim(3, Side::Buy, Price(99), 3)).unwrap();
    engine.submit_limit(lim(4, Side::Sell, Price(102), 4)).unwrap();
    engine.submit_limit(lim(5, Side::Sell, Price(102), 6)).unwrap();

    let snap = engine.snapshot();
    assert_eq!(snap.bids, vec![(Price(100), Qty(12)), (Price(99), Qty(3))]);
    assert_eq!(snap.asks, vec![(Price(102), Qty(10))]);
}