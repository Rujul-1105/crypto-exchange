//! Integration tests for the Phase 3 ledger.
//!
//! The headline test is the canonical double-spend trap from CLAUDE.md
//! Phase 3: "user has 100 USDC, places two orders that would each
//! individually be affordable but together are not — second order must
//! be rejected at lock time, not after both match."
//!
//! All tests run against the in-memory adapter; future Postgres
//! integration is gated behind the `postgres` feature plus a live DB.

use common::{OrderId, OrderKind, Price, Qty, Side, Symbol, Timestamp};
use ledger::{
    Account, Asset, EntryReason, InMemoryLedger, Ledger, LedgerError, OrderStatus, PlaceOrder,
    TradeSettlement, UserId,
};

const ALICE: UserId = UserId(1);
const BOB: UserId = UserId(2);

fn usdc() -> Asset {
    Asset::from("USDC")
}
fn btc() -> Asset {
    Asset::from("BTC")
}
fn btc_usdc() -> Symbol {
    Symbol::from("BTC-USDC")
}

/// `Ledger::place` reads the matching engine's `Price` field as
/// `Option<Price>`. This helper makes constructing limit PlaceOrders
/// less noisy.
fn place_buy(id: u64, user: UserId, qty: u64, price: u64) -> PlaceOrder {
    PlaceOrder {
        id: OrderId(id),
        user,
        symbol: btc_usdc(),
        side: Side::Buy,
        kind: OrderKind::Limit,
        price: Some(Price(price)),
        qty: Qty(qty),
        timestamp: Timestamp(id),
    }
}

fn place_sell(id: u64, user: UserId, qty: u64, price: u64) -> PlaceOrder {
    PlaceOrder {
        id: OrderId(id),
        user,
        symbol: btc_usdc(),
        side: Side::Sell,
        kind: OrderKind::Limit,
        price: Some(Price(price)),
        qty: Qty(qty),
        timestamp: Timestamp(id),
    }
}

// ---- Account lifecycle ----

#[test]
fn empty_account_has_zero_balance() {
    let ledger = InMemoryLedger::new();
    let acct = ledger.account(ALICE, usdc());
    assert_eq!(
        acct,
        Account {
            user: ALICE,
            asset: usdc(),
            available: 0,
            locked: 0,
        }
    );
}

#[test]
fn deposit_credits_available() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    let acct = ledger.account(ALICE, usdc());
    assert_eq!(acct.available, 100);
    assert_eq!(acct.locked, 0);
}

#[test]
fn withdraw_available_debits_only_available() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.withdraw_available(ALICE, usdc(), Qty(30)).unwrap();
    let acct = ledger.account(ALICE, usdc());
    assert_eq!(acct.available, 70);
    assert_eq!(acct.locked, 0);
}

#[test]
fn withdraw_available_below_balance_errors() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(50)).unwrap();
    let result = ledger.withdraw_available(ALICE, usdc(), Qty(60));
    assert!(matches!(result, Err(LedgerError::InsufficientFunds { .. })));
    assert_eq!(ledger.account(ALICE, usdc()).available, 50);
}

#[test]
fn withdraw_available_does_not_touch_locked() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.place(&place_buy(1, ALICE, 10, 5)).unwrap(); // locks 50
    assert_eq!(ledger.account(ALICE, usdc()).available, 50);
    assert_eq!(ledger.account(ALICE, usdc()).locked, 50);
    // Should be able to withdraw_available(49) (within available=50).
    ledger.withdraw_available(ALICE, usdc(), Qty(49)).unwrap();
    assert_eq!(ledger.account(ALICE, usdc()).available, 1);
    assert_eq!(ledger.account(ALICE, usdc()).locked, 50);
    // Should NOT be able to withdraw_available(2) (would draw from locked).
    let result = ledger.withdraw_available(ALICE, usdc(), Qty(2));
    assert!(matches!(result, Err(LedgerError::InsufficientFunds { .. })));
}

// ---- Lock-on-place ----

#[test]
fn place_buy_locks_quote() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    let receipt = ledger.place(&place_buy(1, ALICE, 10, 5)).unwrap();
    assert_eq!(receipt.locked_quote, 50);
    assert_eq!(receipt.locked_base, 0);
    let acct = ledger.account(ALICE, usdc());
    assert_eq!(acct.available, 50);
    assert_eq!(acct.locked, 50);
}

#[test]
fn place_sell_locks_base() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, btc(), Qty(10)).unwrap();
    let receipt = ledger.place(&place_sell(1, ALICE, 5, 100)).unwrap();
    assert_eq!(receipt.locked_base, 5);
    assert_eq!(receipt.locked_quote, 0);
    let acct = ledger.account(ALICE, btc());
    assert_eq!(acct.available, 5);
    assert_eq!(acct.locked, 5);
    let acct_usdc = ledger.account(ALICE, usdc());
    assert_eq!(acct_usdc.available, 0);
    assert_eq!(acct_usdc.locked, 0);
}

#[test]
fn place_insufficient_funds_is_rejected() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(40)).unwrap();
    let result = ledger.place(&place_buy(1, ALICE, 10, 5)); // needs 50
    assert!(matches!(result, Err(LedgerError::InsufficientFunds { .. })));
    // No state change.
    let acct = ledger.account(ALICE, usdc());
    assert_eq!(acct.available, 40);
    assert_eq!(acct.locked, 0);
    // No order was recorded.
    assert!(ledger.order(OrderId(1)).is_none());
}

// ---- THE canonical Phase 3 double-spend test ----

#[test]
fn double_spend_trap() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();

    // Per CLAUDE.md: "user has 100 USDC, places two orders that would
    // each individually be affordable but together are not — second
    // order must be rejected at lock time, not after both match."
    //
    // Each order is 60 USDC (individually affordable; 60 < 100).
    // Together they need 120, which exceeds 100.
    ledger.place(&place_buy(1, ALICE, 12, 5)).unwrap(); // locks 60
    let acct_before = ledger.account(ALICE, usdc());
    assert_eq!(acct_before.available, 40);
    assert_eq!(acct_before.locked, 60);

    let result = ledger.place(&place_buy(2, ALICE, 12, 5));
    assert!(
        matches!(result, Err(LedgerError::InsufficientFunds { .. })),
        "expected InsufficientFunds, got {:?}",
        result
    );

    // Alice's account is exactly as it was after the first order; the
    // second order left no trace.
    let acct_after = ledger.account(ALICE, usdc());
    assert_eq!(acct_after.available, 40);
    assert_eq!(acct_after.locked, 60);
    assert_eq!(acct_after.available + acct_after.locked, 100);
    assert!(ledger.order(OrderId(2)).is_none());
}

#[test]
fn double_spend_trap_three_orders() {
    // Same trap, three orders. After #1 (40 locked), #2 (40 of remaining
    // 60) fits exactly, leaving 20 free. #3 needs 40 — must fail.
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.place(&place_buy(1, ALICE, 4, 10)).unwrap(); // locks 40
    ledger.place(&place_buy(2, ALICE, 4, 10)).unwrap(); // locks 40
    assert_eq!(ledger.account(ALICE, usdc()).available, 20);
    let r = ledger.place(&place_buy(3, ALICE, 4, 10));
    assert!(matches!(r, Err(LedgerError::InsufficientFunds { .. })));
    assert_eq!(ledger.account(ALICE, usdc()).available, 20);
    assert_eq!(ledger.account(ALICE, usdc()).locked, 80);
}

#[test]
fn cancel_releases_lock_back_to_available() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.place(&place_buy(1, ALICE, 10, 5)).unwrap(); // locks 50
    assert_eq!(ledger.account(ALICE, usdc()).available, 50);
    assert_eq!(ledger.account(ALICE, usdc()).locked, 50);

    ledger.cancel(OrderId(1)).unwrap();
    let acct = ledger.account(ALICE, usdc());
    assert_eq!(acct.available, 100);
    assert_eq!(acct.locked, 0);
    assert_eq!(ledger.order(OrderId(1)).unwrap().status, OrderStatus::Cancelled);
}

#[test]
fn double_cancel_is_rejected() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.place(&place_buy(1, ALICE, 10, 5)).unwrap();
    ledger.cancel(OrderId(1)).unwrap();
    let result = ledger.cancel(OrderId(1));
    assert!(matches!(
        result,
        Err(LedgerError::OrderNotCancellable(_))
    ));
}

#[test]
fn cancel_unknown_order_errors() {
    let mut ledger = InMemoryLedger::new();
    let result = ledger.cancel(OrderId(999));
    assert!(matches!(result, Err(LedgerError::UnknownOrder(_))));
}

#[test]
fn duplicate_order_id_rejected() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.place(&place_buy(1, ALICE, 5, 10)).unwrap();
    let result = ledger.place(&place_buy(1, ALICE, 5, 10));
    assert!(matches!(result, Err(LedgerError::DuplicateOrder(_))));
}

// ---- Atomic settlement ----

#[test]
fn settle_trade_moves_funds_atomically() {
    let mut ledger = InMemoryLedger::new();
    // Alice has USDC, places a buy. Bob has BTC, places a sell.
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.deposit(BOB, btc(), Qty(10)).unwrap();

    ledger.place(&place_buy(1, ALICE, 5, 10)).unwrap(); // locks 50 USDC
    ledger.place(&place_sell(2, BOB, 5, 10)).unwrap(); // locks 5 BTC

    // Pre-settlement snapshot.
    let alice_usdc_before = ledger.account(ALICE, usdc());
    let alice_btc_before = ledger.account(ALICE, btc());
    let bob_usdc_before = ledger.account(BOB, usdc());
    let bob_btc_before = ledger.account(BOB, btc());
    assert_eq!(alice_usdc_before, Account { user: ALICE, asset: usdc(), available: 50, locked: 50 });
    assert_eq!(alice_btc_before, Account   { user: ALICE, asset: btc(),  available: 0,  locked: 0  });
    assert_eq!(bob_usdc_before,   Account { user: BOB,   asset: usdc(), available: 0,  locked: 0  });
    assert_eq!(bob_btc_before,    Account { user: BOB,   asset: btc(),  available: 5,  locked: 5  });

    // Settle a trade: Alice buys 3 BTC @ 10 from Bob.
    // Alice: locked[usdc] -= 30, available[btc] += 3
    // Bob:   locked[btc]  -= 3, available[usdc] += 30
    let trade = TradeSettlement {
        symbol: btc_usdc(),
        maker_order_id: OrderId(2),  // Bob is the maker (sell)
        taker_order_id: OrderId(1),  // Alice is the taker (buy)
        price: Price(10),
        qty: Qty(3),
        taker_side: Side::Buy,
    };
    ledger.settle_trade(&trade).unwrap();

    let alice_usdc_after = ledger.account(ALICE, usdc());
    let alice_btc_after  = ledger.account(ALICE, btc());
    let bob_usdc_after   = ledger.account(BOB, usdc());
    let bob_btc_after    = ledger.account(BOB, btc());

    // Alice: 50 locked -> 20 locked, available 50 -> 50 (no change in
    // available USDC; she paid 30 out of locked, so available stays at
    // 50). Wait: the spec is that locked pays for the trade. So:
    //   available[USDC]: 50 (unchanged; she locked 50 of which 30 went
    //     to the trade, 20 is still locked for the unfilled part)
    //   available[BTC]:  0 + 3 = 3
    //   locked[USDC]:    50 - 30 = 20
    //   locked[BTC]:     0
    assert_eq!(alice_usdc_after.available, 50);
    assert_eq!(alice_usdc_after.locked, 20);
    assert_eq!(alice_btc_after.available, 3);
    assert_eq!(alice_btc_after.locked, 0);

    // Bob: locked[BTC] -= 3, available[USDC] += 30
    // Bob's available[BTC] was already reduced at place time (from 10
    // to 5); only locked[BTC] decreases further on the fill.
    assert_eq!(bob_usdc_after.available, 30);
    assert_eq!(bob_usdc_after.locked, 0);
    assert_eq!(bob_btc_after.available, 5); // unchanged from pre-fill
    assert_eq!(bob_btc_after.locked, 5 - 3); // 5 - 3 = 2

    // Conservation: total assets across both users unchanged.
    assert_eq!(alice_usdc_after.total() + bob_usdc_after.total(), 100);
    assert_eq!(alice_btc_after.total() + bob_btc_after.total(), 10);

    // Order rows updated.
    let alice_order = ledger.order(OrderId(1)).unwrap();
    assert_eq!(alice_order.filled_qty, 3);
    assert_eq!(alice_order.status, OrderStatus::Open); // not fully filled
    let bob_order = ledger.order(OrderId(2)).unwrap();
    assert_eq!(bob_order.filled_qty, 3);
    assert_eq!(bob_order.status, OrderStatus::Open);

    // Four ledger entries written with reason = Fill.
    let fills: Vec<_> = ledger
        .entries()
        .iter()
        .filter(|e| e.reason == EntryReason::Fill)
        .collect();
    assert_eq!(fills.len(), 4);
}

#[test]
fn settlement_marks_orders_filled_when_exhausted() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.deposit(BOB, btc(), Qty(10)).unwrap();

    ledger.place(&place_buy(1, ALICE, 5, 10)).unwrap(); // 5 BTC @ 10 = 50 USDC locked
    ledger.place(&place_sell(2, BOB, 5, 10)).unwrap(); // 5 BTC locked

    let trade = TradeSettlement {
        symbol: btc_usdc(),
        maker_order_id: OrderId(2),
        taker_order_id: OrderId(1),
        price: Price(10),
        qty: Qty(5),
        taker_side: Side::Buy,
    };
    ledger.settle_trade(&trade).unwrap();

    assert_eq!(ledger.order(OrderId(1)).unwrap().status, OrderStatus::Filled);
    assert_eq!(ledger.order(OrderId(2)).unwrap().status, OrderStatus::Filled);
}

#[test]
fn settlement_with_unknown_order_errors_and_leaves_no_partial_state() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.place(&place_buy(1, ALICE, 5, 10)).unwrap();

    // Snapshot pre-attempt.
    let alice_usdc_before = ledger.account(ALICE, usdc());
    let alice_btc_before = ledger.account(ALICE, btc());
    let entries_before = ledger.entries().len();

    let trade = TradeSettlement {
        symbol: btc_usdc(),
        maker_order_id: OrderId(999), // doesn't exist
        taker_order_id: OrderId(1),
        price: Price(10),
        qty: Qty(5),
        taker_side: Side::Buy,
    };
    let result = ledger.settle_trade(&trade);
    assert!(matches!(result, Err(LedgerError::UnknownOrder(_))));

    // No partial state was observed. (In-memory atomicity is structural
    // — the test exists to verify the API contract.)
    assert_eq!(ledger.account(ALICE, usdc()), alice_usdc_before);
    assert_eq!(ledger.account(ALICE, btc()), alice_btc_before);
    assert_eq!(ledger.entries().len(), entries_before);
}

#[test]
fn multiple_trades_update_filled_qty_correctly() {
    // First trade: partial fill (3 of 5).
    // Second trade: completes maker's sell, taker's buy partially.
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.deposit(BOB, btc(), Qty(10)).unwrap();

    ledger.place(&place_buy(1, ALICE, 5, 10)).unwrap(); // locks 50
    ledger.place(&place_sell(2, BOB, 5, 10)).unwrap(); // locks 5 BTC

    let trade1 = TradeSettlement {
        symbol: btc_usdc(),
        maker_order_id: OrderId(2),
        taker_order_id: OrderId(1),
        price: Price(10),
        qty: Qty(3),
        taker_side: Side::Buy,
    };
    ledger.settle_trade(&trade1).unwrap();
    assert_eq!(ledger.order(OrderId(1)).unwrap().filled_qty, 3);
    assert_eq!(ledger.order(OrderId(2)).unwrap().filled_qty, 3);

    let trade2 = TradeSettlement {
        symbol: btc_usdc(),
        maker_order_id: OrderId(2),
        taker_order_id: OrderId(1),
        price: Price(10),
        qty: Qty(2),
        taker_side: Side::Buy,
    };
    ledger.settle_trade(&trade2).unwrap();
    assert_eq!(ledger.order(OrderId(1)).unwrap().status, OrderStatus::Filled);
    assert_eq!(ledger.order(OrderId(2)).unwrap().status, OrderStatus::Filled);

    // Alice: paid 50 USDC, got 5 BTC.
    let alice_usdc = ledger.account(ALICE, usdc());
    let alice_btc = ledger.account(ALICE, btc());
    assert_eq!(alice_usdc.available, 50);
    assert_eq!(alice_usdc.locked, 0);
    assert_eq!(alice_btc.available, 5);
    assert_eq!(alice_btc.locked, 0);

    // Bob: gave 5 BTC, got 50 USDC.
    let bob_usdc = ledger.account(BOB, usdc());
    let bob_btc = ledger.account(BOB, btc());
    assert_eq!(bob_usdc.available, 50);
    assert_eq!(bob_usdc.locked, 0);
    assert_eq!(bob_btc.available, 5);
    assert_eq!(bob_btc.locked, 0);

    // Conservation: total conserved across both users.
    assert_eq!(alice_usdc.total() + bob_usdc.total(), 100);
    assert_eq!(alice_btc.total() + bob_btc.total(), 10);
}

#[test]
fn multiple_users_multiple_assets_isolated() {
    let mut ledger = InMemoryLedger::new();
    let carol = UserId(3);
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.deposit(BOB, btc(), Qty(5)).unwrap();
    ledger.deposit(carol, usdc(), Qty(200)).unwrap();

    assert_eq!(ledger.account(ALICE, usdc()).available, 100);
    assert_eq!(ledger.account(ALICE, btc()).available, 0);
    assert_eq!(ledger.account(BOB, usdc()).available, 0);
    assert_eq!(ledger.account(BOB, btc()).available, 5);
    assert_eq!(ledger.account(carol, usdc()).available, 200);

    // Alice and Bob settle a trade; Carol untouched.
    ledger.place(&place_buy(1, ALICE, 2, 20)).unwrap();
    ledger.place(&place_sell(2, BOB, 2, 20)).unwrap();
    let trade = TradeSettlement {
        symbol: btc_usdc(),
        maker_order_id: OrderId(2),
        taker_order_id: OrderId(1),
        price: Price(20),
        qty: Qty(2),
        taker_side: Side::Buy,
    };
    ledger.settle_trade(&trade).unwrap();
    assert_eq!(ledger.account(carol, usdc()).available, 200);
}

// ---- Ledger entries (append-only audit trail) ----

#[test]
fn place_writes_two_ledger_entries() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    let before = ledger.entries().len();
    ledger.place(&place_buy(1, ALICE, 5, 10)).unwrap();
    let new_entries = &ledger.entries()[before..];
    assert_eq!(new_entries.len(), 2);
    assert!(new_entries.iter().all(|e| e.reason == EntryReason::Place));
    assert!(new_entries.iter().any(|e| matches!(e.bucket, ledger::Bucket::Available) && e.delta < 0));
    assert!(new_entries.iter().any(|e| matches!(e.bucket, ledger::Bucket::Locked) && e.delta > 0));
}

#[test]
fn settle_writes_four_ledger_entries_with_trade_id() {
    let mut ledger = InMemoryLedger::new();
    ledger.deposit(ALICE, usdc(), Qty(100)).unwrap();
    ledger.deposit(BOB, btc(), Qty(10)).unwrap();
    ledger.place(&place_buy(1, ALICE, 5, 10)).unwrap();
    ledger.place(&place_sell(2, BOB, 5, 10)).unwrap();

    let before = ledger.entries().len();
    let trade = TradeSettlement {
        symbol: btc_usdc(),
        maker_order_id: OrderId(2),
        taker_order_id: OrderId(1),
        price: Price(10),
        qty: Qty(3),
        taker_side: Side::Buy,
    };
    ledger.settle_trade(&trade).unwrap();
    let new_entries = &ledger.entries()[before..];
    assert_eq!(new_entries.len(), 4);
    assert!(new_entries.iter().all(|e| e.reason == EntryReason::Fill));
    // All four entries share the same trade_id.
    let trade_id = new_entries[0].trade_id.expect("entry has trade_id");
    assert!(new_entries.iter().all(|e| e.trade_id == Some(trade_id)));
}