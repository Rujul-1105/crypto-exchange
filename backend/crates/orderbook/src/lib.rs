//! Orderbook server — the stateful half of the exchange.
//!
//! Public modules:
//! - `engine` — pure matching engine, no I/O
//! - `orderbook` — book data structure
//! - `candle` — OHLCV aggregator
//!
//! Phase 4 wires the engine behind actix-web admin endpoints and a Redis
//! Streams consumer. Phase 5 (API server) is the public REST + WS surface;
//! Phase 6 (settler) consumes Fill events.

pub mod candle;
pub mod engine;
pub mod orderbook;

pub use engine::*;
pub use orderbook::*;

pub mod market;
pub mod redis_bus;
pub mod redis_consumer;
pub mod admin;
pub mod bot;
pub mod snapshot;
