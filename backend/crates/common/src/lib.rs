//! Shared types for the exchange backend.
//!
//! Three binaries (`api`, `orderbook`, `settler`) share these types via the
//! `common` crate. Everything that crosses the Redis Streams bridge or the
//! HTTP/WS boundary lives here.
//!
//! Conventions:
//! - All decimals serialize as **strings** on the wire (JSON) for JS-safe
//!   parsing and zero f64 loss.
//! - Order IDs are `u64`, monotonic per `MatchingEngine` (per symbol).
//! - Trade IDs are `u64`, monotonic per `MatchingEngine` (per symbol).
//! - Timestamps are `i64` unix millis (UTC).
//! - User identity is `String` on the matching layer (base58 pubkey), so this
//!   crate has no dependency on `solana-sdk`.

pub mod candle;
pub mod events;
pub mod order;
pub mod side;
pub mod stream;
pub mod trade;
pub mod wire;

pub use candle::*;
pub use events::*;
pub use order::*;
pub use side::*;
pub use stream::*;
pub use trade::*;
pub use wire::*;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Redis stream names — the only bridge between API tier and orderbook.
pub const STREAM_ORDERS_INCOMING: &str = "orders:incoming";
pub const STREAM_EVENTS_OUTGOING: &str = "events:outgoing";
pub const STREAM_SETTLE_UPDATES: &str = "settle:updates";

pub const DEFAULT_SYMBOL: &str = "SOL-USDC";
