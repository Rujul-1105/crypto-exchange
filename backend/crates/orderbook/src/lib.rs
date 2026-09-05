//! Orderbook server — the stateful half of the exchange.
//!
//! Public modules:
//! - `engine` — pure matching engine, no I/O
//! - `orderbook` — book data structure
//! - `candle` — OHLCV aggregator
//!

pub mod candle;
pub mod engine;
pub mod orderbook;

pub use engine::*;
pub use orderbook::*;

pub mod admin;
pub mod bot;
pub mod market;
pub mod redis_bus;
pub mod redis_consumer;
pub mod snapshot;
