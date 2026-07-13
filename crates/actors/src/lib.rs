//! Per-symbol actor model wrapping the matching engine.
//!
//! ## Phase 2 scope
//!
//! This crate provides [`EngineActor`], a per-symbol task that owns a
//! `MatchingEngine` and processes commands serially via `mpsc` +
//! `oneshot`. Multiple symbols get multiple actors; nothing is shared
//! across symbols except the immutable routing metadata (which the
//! caller maintains — out of scope for this crate).
//!
//! ## Why a separate crate
//!
//! The matching engine must remain async-free (Phase 0 invariant:
//! `matching-engine` depends only on `common`). The actor wrapper is
//! where tokio lives; isolating it in `actors` keeps the engine pure.

mod actor;
mod command;

pub use actor::{ActorError, EngineActor, DEFAULT_BUFFER};
pub use matching_engine::{EngineError, MatchResult, Snapshot, Trade};
pub use common::{Order, OrderId, Price, Qty, Side, Symbol, Timestamp};
pub use tokio::task::JoinHandle;