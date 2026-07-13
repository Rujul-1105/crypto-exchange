//! Postgres-backed [`Ledger`](crate::Ledger) implementation.
//!
//! ## Phase 3 status: STUB
//!
//! This file exists to lock in the future shape and document the
//! transactional approach. The runtime implementation lands in a
//! follow-up phase that wires `sqlx` against a CI Postgres (e.g. via
//! `docker compose up postgres` in `cargo test`). Until then the
//! in-memory adapter in [`memory`](crate::memory) covers the test
//! suite, and the DDL in `migrations/20240101000000_initial.sql`
//! defines the schema this stub will populate.
//!
//! ## What the future impl must guarantee
//!
//! Each mutating method must run inside a single transaction so a
//! crash mid-settlement never produces observable partial state. The
//! pattern (in pseudo-Rust):
//!
//! ```ignore
//! async fn settle_trade(&mut self, trade: &TradeSettlement) -> Result<()> {
//!     let mut tx = self.pool.begin().await?;
//!     // SELECT ... FOR UPDATE on the two orders' rows + the affected
//!     // accounts;
//!     // UPDATE accounts SET available = ... , locked = ... ;
//!     // INSERT INTO ledger_entries (...) ;
//!     // UPDATE orders SET filled_qty = ..., status = ... ;
//!     // INSERT INTO trades (...) ;
//!     tx.commit().await
//! }
//! ```
//!
//! Concurrency safety comes from `SELECT ... FOR UPDATE` on the
//! accounts being debited. In-memory atomicity is `&mut self`; Postgres
//! uses serializable or row-level locks to get the same guarantees.

use common::OrderId;

use crate::error::LedgerError;
use crate::model::{
    Account, Amount, Asset, OrderRow, PlaceOrder, PlaceReceipt, TradeSettlement, UserId,
};

/// Placeholder. Will hold a `sqlx::PgPool` and migration handle.
#[derive(Debug)]
pub struct PostgresLedger {
    // pool: sqlx::PgPool,
    _placeholder: (),
}

impl PostgresLedger {
    /// Construct from a DSN. Will become `pub async fn connect(url: &str)`.
    pub fn connect(_url: &str) -> Result<Self, LedgerError> {
        Err(LedgerError::Internal(
            "PostgresLedger is a Phase 3 stub; runtime impl lands in a follow-up phase".into(),
        ))
    }
}

// All trait methods return LedgerError::Internal to make the stub-ness
// obvious — calling them in Phase 3 is a programming error.
impl crate::Ledger for PostgresLedger {
    fn deposit(
        &mut self,
        _user: UserId,
        _asset: Asset,
        _amount: Amount,
    ) -> Result<(), LedgerError> {
        Err(LedgerError::Internal("PostgresLedger stub".into()))
    }

    fn withdraw_available(
        &mut self,
        _user: UserId,
        _asset: Asset,
        _amount: Amount,
    ) -> Result<(), LedgerError> {
        Err(LedgerError::Internal("PostgresLedger stub".into()))
    }

    fn place(&mut self, _order: &PlaceOrder) -> Result<PlaceReceipt, LedgerError> {
        Err(LedgerError::Internal("PostgresLedger stub".into()))
    }

    fn cancel(&mut self, _order_id: OrderId) -> Result<(), LedgerError> {
        Err(LedgerError::Internal("PostgresLedger stub".into()))
    }

    fn settle_trade(&mut self, _trade: &TradeSettlement) -> Result<(), LedgerError> {
        Err(LedgerError::Internal("PostgresLedger stub".into()))
    }

    fn account(&self, _user: UserId, _asset: Asset) -> Account {
        Account::default()
    }

    fn order(&self, _order_id: OrderId) -> Option<OrderRow> {
        None
    }
}