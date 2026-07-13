//! Domain types for the ledger crate.
//!
//! These mirror the rows of the `accounts`, `orders`, `trades`, and
//! `ledger_entries` tables in `migrations/20240101000000_initial.sql`,
//! kept Rust-native so the in-memory and Postgres adapters can share the
//! same shapes.
//!
//! [`UserId`] and [`Asset`] are defined here (rather than in `common`)
//! because the matching engine does not need them — `Order` in `common`
//! is symbol- and user-agnostic by design. Keeping them here lets the
//! ledger evolve without rippling into other crates.

use common::{OrderId, OrderKind, Price, Qty, Side, Timestamp};

/// Identifies a user. In real systems this would tie to an auth table
/// (Phase 4). For Phase 3 the ledger only needs the integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UserId(pub u64);

/// Identifies an asset. Stored as a small string ("USDC", "BTC", etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Asset(pub String);

impl Asset {
    pub const USDC: Asset = Asset(String::new()); // sentinel — see below

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Asset {
    fn from(s: &str) -> Self {
        Asset(s.to_owned())
    }
}

impl From<String> for Asset {
    fn from(s: String) -> Self {
        Asset(s)
    }
}

impl std::fmt::Display for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A `(base, quote)` asset pair derived from a `Symbol` like `"BTC-USDC"`.
/// If the symbol contains no `-`, the symbol is treated as the base
/// asset and `USDC` is assumed as the quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetPair {
    pub base: Asset,
    pub quote: Asset,
}

/// Token minor units. Reuse [`Qty`] from `common` since it already has
/// the right semantics and arithmetic impls.
pub type Amount = Qty;

/// Current balance for a `(user, asset)` pair. Both fields are u64 minor
/// units; together they always sum to the user's `total_balance`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Account {
    pub user: UserId,
    pub asset: Asset,
    pub available: u64,
    pub locked: u64,
}

impl Account {
    pub fn total(&self) -> u64 {
        self.available + self.locked
    }
}

/// Input to `Ledger::place`. Mirrors the matching engine's `Order` plus
/// the user_id and original_qty that the engine doesn't know about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceOrder {
    pub id: OrderId,
    pub user: UserId,
    pub symbol: common::Symbol,
    pub side: Side,
    pub kind: OrderKind,
    pub price: Option<Price>,
    pub qty: Qty,
    pub timestamp: Timestamp,
}

impl PlaceOrder {
    /// Reject at construction if the order carries an invalid
    /// price/kind pairing (the matching engine's check is mirrored here
    /// so the ledger doesn't even try to lock funds on a malformed
    /// order).
    pub fn validate(&self) -> Result<(), super::error::LedgerError> {
        use super::error::LedgerError;
        match (self.kind, self.price) {
            (OrderKind::Market, Some(_)) => {
                Err(LedgerError::Internal("market order has price".into()))
            }
            (OrderKind::Limit, None) => Err(LedgerError::Internal(
                "limit order missing price".into(),
            )),
            _ => {
                if self.qty.0 == 0 {
                    return Err(LedgerError::Internal("zero qty".into()));
                }
                Ok(())
            }
        }
    }
}

/// Outcome of a successful `place`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaceReceipt {
    pub order_id: OrderId,
    pub locked_quote: u64, // 0 if the order is a sell (locks base, not quote)
    pub locked_base: u64,  // 0 if the order is a buy
}

/// A trade that the matching engine produced, ready to be settled.
/// `taker_side` distinguishes buy vs sell so the settlement knows which
/// side locks quote vs base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeSettlement {
    pub symbol: common::Symbol,
    pub maker_order_id: OrderId,
    pub taker_order_id: OrderId,
    pub price: Price,
    pub qty: Qty,
    pub taker_side: Side,
}

/// Internal ledger row representing an order's persisted state. The
/// matching engine's `Order` has `qty` as remaining; here `qty` is the
/// ORIGINAL placement and `filled_qty` is the cumulative filled amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRow {
    pub id: OrderId,
    pub user: UserId,
    pub symbol: common::Symbol,
    pub side: Side,
    pub kind: OrderKind,
    pub price: Option<Price>,
    pub qty: u64,
    pub filled_qty: u64,
    pub status: OrderStatus,
}

impl OrderRow {
    pub fn remaining(&self) -> u64 {
        self.qty - self.filled_qty
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus {
    Open,
    Filled,
    Cancelled,
    Rejected,
}

/// A single append-only ledger entry, mirroring the `ledger_entries`
/// table. Exposed for tests and for the future admin/reporting path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub user: UserId,
    pub asset: Asset,
    pub bucket: Bucket,
    pub delta: i64, // signed
    pub reason: EntryReason,
    pub order_id: Option<OrderId>,
    pub trade_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Available,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryReason {
    Deposit,
    Withdraw,
    Place,
    Cancel,
    Fill,
}

impl EntryReason {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryReason::Deposit => "deposit",
            EntryReason::Withdraw => "withdraw",
            EntryReason::Place => "place",
            EntryReason::Cancel => "cancel",
            EntryReason::Fill => "fill",
        }
    }
}