//! Double-entry ledger with atomic settlement.
//!
//! ## Phase 3 scope
//!
//! Account model: `available_balance` + `locked_balance` per
//! `(user, asset)`. Place locks funds (available → locked). Settle moves
//! locked → available (or out) atomically across both sides of a trade.
//!
//! The [`Ledger`] trait is the public API; [`InMemoryLedger`] is the
//! fully-tested implementation; [`PostgresLedger`] is a stub whose
//! schema lives in `migrations/20240101000000_initial.sql`.
//!
//! ## Atomicity
//!
//! In-memory atomicity is `&mut self`: each operation either completes
//! fully or returns `Err` with no state change. The Postgres adapter
//! (future) uses a single `BEGIN; ... COMMIT;` per operation.
//!
//! ## Append-only `ledger_entries`
//!
//! Every balance change writes a row. The Postgres schema enforces
//! append-only via a trigger; the in-memory adapter exposes entries via
//! [`InMemoryLedger::entries`].

pub mod error;
pub mod memory;
pub mod model;
pub mod postgres;

pub use error::LedgerError;
pub use memory::InMemoryLedger;
pub use model::{
    Account, Amount, Asset, AssetPair, Bucket, EntryReason, LedgerEntry, OrderRow, OrderStatus,
    PlaceOrder, PlaceReceipt, TradeSettlement, UserId,
};
pub use postgres::PostgresLedger;

// Re-export `OrderId` from common so trait signatures can name it.
pub use common::OrderId;

/// The public API every ledger backend must satisfy.
///
/// Sync on purpose: `InMemoryLedger` is sync, and `PostgresLedger` (when
/// it lands) can either block on its connection or `spawn_blocking` its
/// async work. Either way the test suite uses the same surface.
pub trait Ledger {
    /// Credit `amount` of `asset` to `user`'s `available` balance.
    fn deposit(
        &mut self,
        user: UserId,
        asset: Asset,
        amount: Amount,
    ) -> Result<(), LedgerError>;

    /// Debit `amount` from `user`'s `available` balance. Errors with
    /// [`InsufficientFunds`](LedgerError::InsufficientFunds) if not
    /// enough. **Never touches `locked`.**
    fn withdraw_available(
        &mut self,
        user: UserId,
        asset: Asset,
        amount: Amount,
    ) -> Result<(), LedgerError>;

    /// Place an order. Locks the required funds
    /// (`available → locked`) atomically. Errors with
    /// [`InsufficientFunds`](LedgerError::InsufficientFunds) if the
    /// user can't afford it. The canonical Phase 3 double-spend test
    /// relies on this returning an error rather than allowing a
    /// partial lock.
    fn place(&mut self, order: &PlaceOrder) -> Result<PlaceReceipt, LedgerError>;

    /// Cancel an open order. Releases any unfilled locked funds back
    /// to available. Errors if the order isn't `open`.
    fn cancel(&mut self, order_id: OrderId) -> Result<(), LedgerError>;

    /// Settle a single trade atomically. Updates both sides' balances,
    /// writes 4 ledger entries, increments `filled_qty` on both orders,
    /// marks them `filled` when exhausted. The whole operation either
    /// completes or rolls back.
    fn settle_trade(&mut self, trade: &TradeSettlement) -> Result<(), LedgerError>;

    /// Current balance for `(user, asset)`. Returns zeroed default if
    /// no entry exists.
    fn account(&self, user: UserId, asset: Asset) -> Account;

    /// Look up an order's persisted row by id.
    fn order(&self, order_id: OrderId) -> Option<OrderRow>;
}

// Re-export the matching engine's `Order` so callers have a single
// import path; the ledger crate builds its own `PlaceOrder` on top.
pub use common::Order;