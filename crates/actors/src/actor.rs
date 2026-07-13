//! Per-symbol actor wrapping the matching engine.
//!
//! The actor is the sole owner of its `MatchingEngine`. Commands arrive
//! over an `mpsc` channel and are processed strictly in send order, which
//! guarantees per-symbol FIFO ordering for free. Each command's response
//! is delivered through a dedicated `oneshot` channel so multiple
//! commands can be in flight concurrently without coupling response order
//! to request order on the caller side.
//!
//! ## Shutdown
//!
//! Dropping every `EngineActor` clone closes the sender. The actor task
//! observes `rx.recv()` returning `None` and exits, dropping the engine.
//! No explicit shutdown API.

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use common::{Order, OrderId, Price, Symbol};
use matching_engine::{EngineError, MatchResult, MatchingEngine, Snapshot};

use crate::command::Command;

/// Default channel buffer size. With this buffer the actor can hold
/// 1024 commands in its inbox before senders start awaiting backpressure.
pub const DEFAULT_BUFFER: usize = 1024;

/// A handle to a per-symbol matching engine running on its own tokio task.
///
/// `EngineActor` is cheap to clone (it wraps an `mpsc::Sender`, which is
/// internally reference-counted). Clones share the same actor; the
/// matching engine is mutated only inside the actor's task.
#[derive(Clone)]
pub struct EngineActor {
    symbol: Symbol,
    sender: mpsc::Sender<Command>,
}

impl EngineActor {
    /// Spawn a new actor for `symbol` with the default channel buffer.
    pub fn spawn(symbol: Symbol) -> (Self, JoinHandle<()>) {
        Self::spawn_with_buffer(symbol, DEFAULT_BUFFER)
    }

    /// Spawn a new actor with a custom channel buffer.
    ///
    /// Smaller buffers give tighter backpressure under flood; larger
    /// buffers let the actor keep up with bursts at the cost of memory.
    pub fn spawn_with_buffer(symbol: Symbol, buffer: usize) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(buffer);
        let handle = tokio::spawn(actor_task(symbol.clone(), rx));
        (EngineActor { symbol, sender: tx }, handle)
    }

    /// The symbol this actor is dedicated to.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Submit a limit order. Awaits the actor's response.
    pub async fn submit_limit(&self, order: Order) -> Result<MatchResult, ActorError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send(Command::SubmitLimit { order, resp: resp_tx })
            .await
            .map_err(|_| ActorError::SendFailed)?;
        resp_rx
            .await
            .map_err(|_| ActorError::RecvFailed)?
            .map_err(ActorError::Engine)
    }

    /// Submit a market order. Awaits the actor's response.
    pub async fn submit_market(&self, order: Order) -> Result<MatchResult, ActorError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send(Command::SubmitMarket { order, resp: resp_tx })
            .await
            .map_err(|_| ActorError::SendFailed)?;
        resp_rx
            .await
            .map_err(|_| ActorError::RecvFailed)?
            .map_err(ActorError::Engine)
    }

    /// Cancel a resting order by id. Awaits the actor's response.
    pub async fn cancel(&self, id: OrderId) -> Result<(), ActorError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send(Command::Cancel { id, resp: resp_tx })
            .await
            .map_err(|_| ActorError::SendFailed)?;
        resp_rx
            .await
            .map_err(|_| ActorError::RecvFailed)?
            .map_err(ActorError::Engine)
    }

    /// Take an aggregated snapshot of the book.
    pub async fn snapshot(&self) -> Snapshot {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send(Command::Snapshot { resp: resp_tx })
            .await
            .expect("actor task is alive while we hold a sender");
        resp_rx.await.expect("actor must send a snapshot")
    }

    /// Best bid price, or `None` if no bids.
    pub async fn best_bid(&self) -> Option<Price> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send(Command::BestBid { resp: resp_tx })
            .await
            .expect("actor task is alive while we hold a sender");
        resp_rx.await.expect("actor must send a best_bid")
    }

    /// Best ask price, or `None` if no asks.
    pub async fn best_ask(&self) -> Option<Price> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send(Command::BestAsk { resp: resp_tx })
            .await
            .expect("actor task is alive while we hold a sender");
        resp_rx.await.expect("actor must send a best_ask")
    }
}

/// Errors returned by the actor wrapper. The two failure modes are:
///   * `Engine(...)` — the underlying matching engine rejected the order
///     or cancel. This is a normal, recoverable error.
///   * `SendFailed` / `RecvFailed` — the actor task is gone (every
///     `EngineActor` clone was dropped, or the task panicked). Caller
///     should treat this as a service-level failure, not a per-order one.
#[derive(Debug)]
pub enum ActorError {
    /// The actor's mpsc channel was closed before our send could be
    /// enqueued. Every `EngineActor` clone has been dropped.
    SendFailed,
    /// The actor task dropped the response sender without sending —
    /// typically a panic in the actor loop.
    RecvFailed,
    /// The matching engine rejected the command (e.g. duplicate id,
    /// unknown cancel target). Wraps the original `EngineError`.
    Engine(EngineError),
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorError::SendFailed => f.write_str("actor task is no longer accepting commands"),
            ActorError::RecvFailed => f.write_str("actor task dropped the response without sending"),
            ActorError::Engine(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ActorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ActorError::Engine(e) => Some(e),
            _ => None,
        }
    }
}

impl From<EngineError> for ActorError {
    fn from(e: EngineError) -> Self {
        ActorError::Engine(e)
    }
}

/// The actor's event loop. Owns the `MatchingEngine` for one symbol;
/// processes commands serially.
async fn actor_task(_symbol: Symbol, mut rx: mpsc::Receiver<Command>) {
    let mut engine = MatchingEngine::new();

    while let Some(cmd) = rx.recv().await {
        match cmd {
            Command::SubmitLimit { order, resp } => {
                let result = engine.submit_limit(order);
                let _ = resp.send(result);
            }
            Command::SubmitMarket { order, resp } => {
                let result = engine.submit_market(order);
                let _ = resp.send(result);
            }
            Command::Cancel { id, resp } => {
                let result = engine.cancel(id);
                let _ = resp.send(result);
            }
            Command::Snapshot { resp } => {
                let snap = engine.snapshot();
                let _ = resp.send(snap);
            }
            Command::BestBid { resp } => {
                let bb = engine.best_bid();
                let _ = resp.send(bb);
            }
            Command::BestAsk { resp } => {
                let ba = engine.best_ask();
                let _ = resp.send(ba);
            }
        }
    }
}