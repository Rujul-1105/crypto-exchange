//! Double-entry ledger with atomic settlement.
//!
//! ## Phase 0 status
//!
//! Workspace-member stub. The real implementation arrives in Phase 3, at
//! which point this crate gains:
//!
//! - `available_balance` and `locked_balance` per (user, asset)
//! - Lock-on-place semantics so order placement moves available → locked
//!   before the order reaches the matching engine
//! - Postgres schema for `accounts`, `ledger_entries` (append-only),
//!   `orders`, `trades`
//! - Atomic settlement: debit/credit both sides, mark order status, insert
//!   trade record, insert ledger entries — all in a single DB transaction
//! - The explicit double-spend test from CLAUDE.md Phase 3
//!
//! Do not stub out API handlers in this crate. The ledger is a correctness
//! boundary; its public API should reflect what the persistence layer
//! actually guarantees.

/// Placeholder for Phase 0. Real types arrive in Phase 3.
#[derive(Debug, Clone, Copy)]
pub struct Ledger;

impl Ledger {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_constructs() {
        let _ledger = Ledger::new();
    }
}