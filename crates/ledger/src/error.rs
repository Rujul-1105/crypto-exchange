//! Errors returned by the [`Ledger`](crate::Ledger) trait.

use common::OrderId;

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// Place rejected because the user doesn't have enough available
    /// balance to lock the order's notional.
    #[error("insufficient available balance for user {user:?} asset {asset}: required {required}, available {available}")]
    InsufficientFunds {
        user: crate::model::UserId,
        asset: crate::model::Asset,
        required: u64,
        available: u64,
    },

    /// Cancel or settlement targeted an order id that is not recorded as
    /// placed (or was already cancelled, rejected, or fully filled).
    #[error("unknown order: {0:?}")]
    UnknownOrder(OrderId),

    /// Cancel targeted an order whose status doesn't permit cancellation
    /// (already cancelled, already filled, or already rejected).
    #[error("order {0:?} is not in a cancellable state")]
    OrderNotCancellable(OrderId),

    /// Place called twice with the same order id. The ledger enforces
    /// uniqueness the same way the matching engine does.
    #[error("duplicate order id: {0:?}")]
    DuplicateOrder(OrderId),

    /// Settlement tried to fill more than the remaining qty on one side
    /// of a trade. Defensive; should never fire in practice because the
    /// matching engine enforces it.
    #[error("trade would overfill an order: order {order:?}, qty={qty}, remaining={remaining}")]
    TradeWouldOverfill {
        order: OrderId,
        qty: u64,
        remaining: u64,
    },

    /// `qty * price` (or similar) overflowed u64. With reasonable
    /// per-asset scale factors this is vanishingly unlikely; we treat it
    /// as a hard error rather than a panic.
    #[error("arithmetic overflow in {op}: {a} * {b}")]
    Overflow { op: &'static str, a: u64, b: u64 },

    /// Catch-all for backend errors. Postgres adapter uses this for
    /// connection / SQL errors.
    #[error("internal ledger error: {0}")]
    Internal(String),
}