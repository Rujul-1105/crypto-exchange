//! In-memory implementation of the [`Ledger`](crate::Ledger) trait.
//!
//! This is the canonical implementation used by the Phase 3 test suite.
//! It guarantees atomicity by holding `&mut self` for the duration of
//! every mutating operation. The Postgres adapter will guarantee the
//! same with `BEGIN; ... COMMIT;`.

use std::collections::HashMap;

use common::{OrderId, OrderKind, Price, Qty, Side, Symbol};

use crate::error::LedgerError;
use crate::model::{
    Account, Asset, AssetPair, Bucket, EntryReason, LedgerEntry, OrderRow, OrderStatus,
    PlaceOrder, PlaceReceipt, TradeSettlement, UserId,
};

/// In-memory ledger. Each instance owns its own state; no global locks.
#[derive(Debug, Default)]
pub struct InMemoryLedger {
    /// `(user, asset) -> Account`. Absent entry = zero balance.
    accounts: HashMap<(UserId, Asset), Account>,
    /// `order_id -> OrderRow`. Absent entry = never placed.
    orders: HashMap<OrderId, OrderRow>,
    /// Append-only log of every ledger entry made so far. Kept for
    /// tests + admin use; not consulted by the matching logic.
    entries: Vec<LedgerEntry>,
    /// Auto-incrementing trade id. Postgres would use BIGSERIAL.
    next_trade_id: u64,
}

impl InMemoryLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// All ledger entries written so far. Useful for tests + audit.
    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    /// All order rows currently recorded.
    pub fn orders(&self) -> impl Iterator<Item = &OrderRow> {
        self.orders.values()
    }

    // ---- internal helpers ----

    fn balance_of(&self, user: UserId, asset: &Asset) -> Account {
        self.accounts
            .get(&(user, asset.clone()))
            .cloned()
            .unwrap_or(Account {
                user,
                asset: asset.clone(),
                available: 0,
                locked: 0,
            })
    }

    fn apply_delta(
        &mut self,
        user: UserId,
        asset: Asset,
        bucket: Bucket,
        delta: i64,
        reason: EntryReason,
        order_id: Option<OrderId>,
        trade_id: Option<u64>,
    ) -> Result<(), LedgerError> {
        let entry = self
            .accounts
            .entry((user, asset.clone()))
            .or_insert(Account {
                user,
                asset: asset.clone(),
                available: 0,
                locked: 0,
            });
        match bucket {
            Bucket::Available => {
                apply_u64_delta(&mut entry.available, delta)
                    .map_err(|_| LedgerError::Internal(format!(
                        "available balance underflow for {:?}/{}",
                        user, asset
                    )))?;
            }
            Bucket::Locked => {
                apply_u64_delta(&mut entry.locked, delta).map_err(|_| {
                    LedgerError::Internal(format!(
                        "locked balance underflow for {:?}/{}",
                        user, asset
                    ))
                })?;
            }
        }
        self.entries.push(LedgerEntry {
            user,
            asset,
            bucket,
            delta,
            reason,
            order_id,
            trade_id,
        });
        Ok(())
    }

    /// Resolve the asset pair for a symbol. Convention: `BASE-QUOTE`
    /// splits on the first `-`. If the symbol contains no `-`, the
    /// whole string is the base and `USDC` is the quote.
    fn pair_for(symbol: &Symbol) -> AssetPair {
        if let Some(idx) = symbol.as_str().find('-') {
            AssetPair {
                base: Asset(symbol.as_str()[..idx].to_owned()),
                quote: Asset(symbol.as_str()[idx + 1..].to_owned()),
            }
        } else {
            AssetPair {
                base: Asset(symbol.as_str().to_owned()),
                quote: Asset("USDC".to_owned()),
            }
        }
    }
}

fn apply_u64_delta(target: &mut u64, delta: i64) -> Result<(), ()> {
    if delta >= 0 {
        *target = target.checked_add(delta as u64).ok_or(())?;
    } else {
        let abs = delta.unsigned_abs();
        if *target < abs {
            return Err(());
        }
        *target -= abs;
    }
    Ok(())
}

impl crate::Ledger for InMemoryLedger {
    fn deposit(
        &mut self,
        user: UserId,
        asset: Asset,
        amount: crate::model::Amount,
    ) -> Result<(), LedgerError> {
        if amount.0 == 0 {
            return Err(LedgerError::Internal("deposit with zero amount".into()));
        }
        self.apply_delta(
            user,
            asset,
            Bucket::Available,
            amount.0 as i64,
            EntryReason::Deposit,
            None,
            None,
        )
    }

    fn withdraw_available(
        &mut self,
        user: UserId,
        asset: Asset,
        amount: crate::model::Amount,
    ) -> Result<(), LedgerError> {
        let acct = self.balance_of(user, &asset);
        if amount.0 > acct.available {
            return Err(LedgerError::InsufficientFunds {
                user,
                asset,
                required: amount.0,
                available: acct.available,
            });
        }
        self.apply_delta(
            user,
            asset,
            Bucket::Available,
            -(amount.0 as i64),
            EntryReason::Withdraw,
            None,
            None,
        )
    }

    fn place(&mut self, order: &PlaceOrder) -> Result<PlaceReceipt, LedgerError> {
        order.validate()?;

        if self.orders.contains_key(&order.id) {
            return Err(LedgerError::DuplicateOrder(order.id));
        }

        let pair = Self::pair_for(&order.symbol);
        let price = order.price.expect("Limit orders have a price (validated)");
        let notional = (order.qty.0)
            .checked_mul(price.0)
            .ok_or(LedgerError::Overflow {
                op: "qty*price",
                a: order.qty.0,
                b: price.0,
            })?;

        match order.side {
            Side::Buy => {
                let avail = self.balance_of(order.user, &pair.quote);
                if avail.available < notional {
                    return Err(LedgerError::InsufficientFunds {
                        user: order.user,
                        asset: pair.quote.clone(),
                        required: notional,
                        available: avail.available,
                    });
                }
                // available[quote] -= notional, locked[quote] += notional
                self.apply_delta(
                    order.user,
                    pair.quote.clone(),
                    Bucket::Available,
                    -(notional as i64),
                    EntryReason::Place,
                    Some(order.id),
                    None,
                )?;
                self.apply_delta(
                    order.user,
                    pair.quote.clone(),
                    Bucket::Locked,
                    notional as i64,
                    EntryReason::Place,
                    Some(order.id),
                    None,
                )?;
            }
            Side::Sell => {
                let avail = self.balance_of(order.user, &pair.base);
                if avail.available < order.qty.0 {
                    return Err(LedgerError::InsufficientFunds {
                        user: order.user,
                        asset: pair.base.clone(),
                        required: order.qty.0,
                        available: avail.available,
                    });
                }
                self.apply_delta(
                    order.user,
                    pair.base.clone(),
                    Bucket::Available,
                    -(order.qty.0 as i64),
                    EntryReason::Place,
                    Some(order.id),
                    None,
                )?;
                self.apply_delta(
                    order.user,
                    pair.base.clone(),
                    Bucket::Locked,
                    order.qty.0 as i64,
                    EntryReason::Place,
                    Some(order.id),
                    None,
                )?;
            }
        }

        self.orders.insert(
            order.id,
            OrderRow {
                id: order.id,
                user: order.user,
                symbol: order.symbol.clone(),
                side: order.side,
                kind: order.kind,
                price: order.price,
                qty: order.qty.0,
                filled_qty: 0,
                status: OrderStatus::Open,
            },
        );

        Ok(PlaceReceipt {
            order_id: order.id,
            locked_quote: match order.side {
                Side::Buy => notional,
                Side::Sell => 0,
            },
            locked_base: match order.side {
                Side::Buy => 0,
                Side::Sell => order.qty.0,
            },
        })
    }

    fn cancel(&mut self, order_id: OrderId) -> Result<(), LedgerError> {
        let mut order = self
            .orders
            .remove(&order_id)
            .ok_or(LedgerError::UnknownOrder(order_id))?;

        if order.status != OrderStatus::Open {
            // We already removed it; restore and report.
            self.orders.insert(order_id, order);
            return Err(LedgerError::OrderNotCancellable(order_id));
        }

        let pair = Self::pair_for(&order.symbol);
        let remaining = order.remaining();
        if remaining > 0 {
            // Release locked → available. The asset depends on side.
            match order.side {
                Side::Buy => {
                    let price = order.price.expect("limit buy has price");
                    let notional = (remaining)
                        .checked_mul(price.0)
                        .ok_or(LedgerError::Overflow {
                            op: "remaining*price",
                            a: remaining,
                            b: price.0,
                        })?;
                    self.apply_delta(
                        order.user,
                        pair.quote.clone(),
                        Bucket::Locked,
                        -(notional as i64),
                        EntryReason::Cancel,
                        Some(order_id),
                        None,
                    )?;
                    self.apply_delta(
                        order.user,
                        pair.quote.clone(),
                        Bucket::Available,
                        notional as i64,
                        EntryReason::Cancel,
                        Some(order_id),
                        None,
                    )?;
                }
                Side::Sell => {
                    self.apply_delta(
                        order.user,
                        pair.base.clone(),
                        Bucket::Locked,
                        -(remaining as i64),
                        EntryReason::Cancel,
                        Some(order_id),
                        None,
                    )?;
                    self.apply_delta(
                        order.user,
                        pair.base.clone(),
                        Bucket::Available,
                        remaining as i64,
                        EntryReason::Cancel,
                        Some(order_id),
                        None,
                    )?;
                }
            }
        }

        order.status = OrderStatus::Cancelled;
        // Re-insert with cancelled status. (We removed it above to take
        // ownership of `filled_qty` semantics; we put it back as a
        // tombstone.)
        self.orders.insert(order_id, order);
        Ok(())
    }

    fn settle_trade(
        &mut self,
        trade: &TradeSettlement,
    ) -> Result<(), LedgerError> {
        let symbol = trade.symbol.clone();
        let pair = Self::pair_for(&symbol);
        let price = trade.price.0;
        let qty = trade.qty.0;
        let notional = qty.checked_mul(price).ok_or(LedgerError::Overflow {
            op: "qty*price",
            a: qty,
            b: price,
        })?;

        // Look up both orders. We clone what we need out, then drop the
        // immutable borrow before mutating.
        let maker = self
            .orders
            .get(&trade.maker_order_id)
            .cloned()
            .ok_or(LedgerError::UnknownOrder(trade.maker_order_id))?;
        let taker = self
            .orders
            .get(&trade.taker_order_id)
            .cloned()
            .ok_or(LedgerError::UnknownOrder(trade.taker_order_id))?;

        // Defensive: confirm sides align (maker_sell <-> taker_buy; or
        // the reverse).
        match (maker.side, trade.taker_side) {
            (Side::Sell, Side::Buy) | (Side::Buy, Side::Sell) => {}
            _ => {
                return Err(LedgerError::Internal(
                    "settle_trade sides inconsistent with taker_side".into(),
                ));
            }
        }

        // Pre-check: settle_trade is a single atomic operation; if ANY
        // sub-step would underflow the locked balance, abort the whole
        // thing and return an error rather than partially writing.
        let maker_locked = self.balance_of(maker.user, &maker_locked_asset(maker.side, &pair));
        let taker_locked =
            self.balance_of(taker.user, &taker_locked_asset(taker.side, &pair));
        if maker_locked.locked < qty {
            return Err(LedgerError::Internal(format!(
                "maker locked underflow: user {:?} has {} < {}",
                maker.user, maker_locked.locked, qty
            )));
        }
        if taker_locked.locked < notional {
            return Err(LedgerError::Internal(format!(
                "taker locked underflow: user {:?} has {} < {}",
                taker.user, taker_locked.locked, notional
            )));
        }
        // In a real DB this would also guard against overfilling an
        // order; here we do it explicitly because it's cheap.
        let maker_remaining = maker.qty - maker.filled_qty;
        let taker_remaining = taker.qty - taker.filled_qty;
        if qty > maker_remaining || qty > taker_remaining {
            return Err(LedgerError::TradeWouldOverfill {
                order: if qty > maker_remaining {
                    trade.maker_order_id
                } else {
                    trade.taker_order_id
                },
                qty,
                remaining: maker_remaining.min(taker_remaining),
            });
        }

        self.next_trade_id += 1;
        let trade_id = self.next_trade_id;

        // ---- Maker side ----
        // Maker locked -> maker available (seller receives quote; buyer
        // gives up the locked base). The exact mechanics depend on the
        // maker's side:
        //   * Maker is seller: locked[base] -= qty; available[quote] += notional
        //   * Maker is buyer:  locked[quote] -= notional; available[base] += qty
        match maker.side {
            Side::Sell => {
                self.apply_delta(
                    maker.user,
                    pair.base.clone(),
                    Bucket::Locked,
                    -(qty as i64),
                    EntryReason::Fill,
                    Some(trade.maker_order_id),
                    Some(trade_id),
                )?;
                self.apply_delta(
                    maker.user,
                    pair.quote.clone(),
                    Bucket::Available,
                    notional as i64,
                    EntryReason::Fill,
                    Some(trade.maker_order_id),
                    Some(trade_id),
                )?;
            }
            Side::Buy => {
                self.apply_delta(
                    maker.user,
                    pair.quote.clone(),
                    Bucket::Locked,
                    -(notional as i64),
                    EntryReason::Fill,
                    Some(trade.maker_order_id),
                    Some(trade_id),
                )?;
                self.apply_delta(
                    maker.user,
                    pair.base.clone(),
                    Bucket::Available,
                    qty as i64,
                    EntryReason::Fill,
                    Some(trade.maker_order_id),
                    Some(trade_id),
                )?;
            }
        }

        // ---- Taker side ----
        // Taker is the *incoming* side; its locked balance already
        // matches `taker_side`. Mirror of the maker logic.
        match trade.taker_side {
            Side::Buy => {
                self.apply_delta(
                    taker.user,
                    pair.quote.clone(),
                    Bucket::Locked,
                    -(notional as i64),
                    EntryReason::Fill,
                    Some(trade.taker_order_id),
                    Some(trade_id),
                )?;
                self.apply_delta(
                    taker.user,
                    pair.base.clone(),
                    Bucket::Available,
                    qty as i64,
                    EntryReason::Fill,
                    Some(trade.taker_order_id),
                    Some(trade_id),
                )?;
            }
            Side::Sell => {
                self.apply_delta(
                    taker.user,
                    pair.base.clone(),
                    Bucket::Locked,
                    -(qty as i64),
                    EntryReason::Fill,
                    Some(trade.taker_order_id),
                    Some(trade_id),
                )?;
                self.apply_delta(
                    taker.user,
                    pair.quote.clone(),
                    Bucket::Available,
                    notional as i64,
                    EntryReason::Fill,
                    Some(trade.taker_order_id),
                    Some(trade_id),
                )?;
            }
        }

        // Update filled_qty on both orders; mark filled if exhausted.
        // We re-borrow self.orders mutably. The previous get/copied above
        // already released its borrow, so this is safe.
        let update_filled = |orders: &mut HashMap<OrderId, OrderRow>,
                            id: OrderId,
                            add: u64|
         -> Result<(), LedgerError> {
            let order = orders
                .get_mut(&id)
                .ok_or(LedgerError::UnknownOrder(id))?;
            order.filled_qty += add;
            if order.filled_qty == order.qty {
                order.status = OrderStatus::Filled;
            }
            Ok(())
        };
        update_filled(&mut self.orders, trade.maker_order_id, qty)?;
        update_filled(&mut self.orders, trade.taker_order_id, qty)?;

        Ok(())
    }

    fn account(&self, user: UserId, asset: Asset) -> Account {
        self.balance_of(user, &asset)
    }

    fn order(&self, order_id: OrderId) -> Option<OrderRow> {
        self.orders.get(&order_id).cloned()
    }
}

fn maker_locked_asset(side: Side, pair: &AssetPair) -> Asset {
    match side {
        Side::Buy => pair.quote.clone(),
        Side::Sell => pair.base.clone(),
    }
}

fn taker_locked_asset(side: Side, pair: &AssetPair) -> Asset {
    match side {
        Side::Buy => pair.quote.clone(),
        Side::Sell => pair.base.clone(),
    }
}

// Compile-time sanity: ensure `OrderKind`/`Price`/`Qty`/`Side`/`Symbol`
// are reachable from this module so we can reference them in helpers.
#[allow(dead_code)]
fn _kinds_used() {
    let _: OrderKind = OrderKind::Limit;
    let _: Price = Price(0);
    let _: Qty = Qty(0);
    let _: Side = Side::Buy;
    let _: Symbol = Symbol::from("");
}