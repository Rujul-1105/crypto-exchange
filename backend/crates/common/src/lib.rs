//! Shared types and constants for the exchange backend.
//!
//! Phase 3 fills this in with Order, Trade, Side, OrderType, EngineEvent, and
//! Redis wire messages. For now it is a placeholder so the workspace compiles.

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
