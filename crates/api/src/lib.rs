//! HTTP API surface for the exchange.
//!
//! ## Phase 0 status
//!
//! Workspace-member stub. The real implementation is layered across two
//! phases:
//!
//! - **Phase 4 — Auth & RBAC**: Argon2 password hashing, JWT sessions,
//!   role-based middleware (`user`, `market_maker`, `admin`). Admin-only
//!   endpoints for manual balance adjustment and symbol management.
//! - **Phase 5 — REST & persistence integration**: Axum REST endpoints
//!   (place order, cancel, book snapshot, balances, trade history),
//!   idempotency keys on order submission, per-user rate limiting, and the
//!   event log / WAL that the in-memory book replays on restart to
//!   reconstruct state.
//!
//! Resist the temptation to start writing handlers in this crate during
//! Phase 1 / 2 / 3. The matching engine and ledger need to be correct in
//! isolation first; the API is plumbing on top of correct pieces.

/// Placeholder for Phase 0. Real handlers arrive in Phase 4 / 5.
#[derive(Debug, Clone, Copy)]
pub struct Api;

impl Api {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Api {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_constructs() {
        let _api = Api::new();
    }
}