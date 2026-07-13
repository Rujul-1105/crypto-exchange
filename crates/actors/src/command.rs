//! Internal command type sent over the actor's mpsc channel.
//!
//! Each command carries a oneshot response channel so the caller can await
//! the result. Keeping the response path per-command (rather than reusing
//! a single response channel) lets multiple commands be in flight without
//! coupling response order to request order on the caller side.

use tokio::sync::oneshot;

use common::{Order, OrderId, Price};
use matching_engine::{EngineError, MatchResult, Snapshot};

pub(crate) enum Command {
    SubmitLimit {
        order: Order,
        resp: oneshot::Sender<Result<MatchResult, EngineError>>,
    },
    SubmitMarket {
        order: Order,
        resp: oneshot::Sender<Result<MatchResult, EngineError>>,
    },
    Cancel {
        id: OrderId,
        resp: oneshot::Sender<Result<(), EngineError>>,
    },
    Snapshot {
        resp: oneshot::Sender<Snapshot>,
    },
    BestBid {
        resp: oneshot::Sender<Option<Price>>,
    },
    BestAsk {
        resp: oneshot::Sender<Option<Price>>,
    },
}