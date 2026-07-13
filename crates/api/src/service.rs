//! Order service + per-symbol actor registry.
//!
//! The `OrderService` is the orchestrator: it locks funds in the ledger
//! (`place`), submits the order to the matching engine actor, settles
//! resulting trades back on the ledger, and records the action to the
//! WAL. Replays are done by `bootstrap_from_wal`, which rebuilds the
//! actor registry and re-applies every event.
//!
//! ## Phase 5 scope
//!
//! This is intentionally a thin layer over Phase 1/2/3 — it does no
//! matching logic, no auth (handlers do that before reaching here),
//! and no HTTP. Its only job is composition.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use actors::EngineActor;
use common::{Order, OrderId, OrderKind, Price, Qty, Side, Symbol, Timestamp};
use ledger::{
    Asset, InMemoryLedger, Ledger, LedgerError, PlaceOrder, PlaceReceipt, TradeSettlement,
    UserId,
};

use crate::idempotency::IdempotencyCache;
use crate::ratelimit::{BucketConfig, RateLimiter};
use crate::wal::{Wal, WalAction, WalEvent};

/// Per-symbol actor registry. Spawns a new actor on first use.
#[derive(Clone)]
pub struct ActorRegistry {
    inner: Arc<Mutex<HashMap<Symbol, EngineActor>>>,
}

impl ActorRegistry {
    pub fn new() -> Self {
        ActorRegistry {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get or create the actor for `symbol`. The returned handle is
    /// `Clone` and can be moved into an async task.
    pub fn get_or_create(&self, symbol: Symbol) -> EngineActor {
        let mut guard = self.inner.lock().expect("actor registry mutex");
        if let Some(actor) = guard.get(&symbol) {
            return actor.clone();
        }
        let (actor, _handle) = EngineActor::spawn(symbol.clone());
        guard.insert(symbol.clone(), actor.clone());
        actor
    }

    /// List of known symbols (for replay / admin queries).
    pub fn symbols(&self) -> Vec<Symbol> {
        let guard = self.inner.lock().expect("actor registry mutex");
        guard.keys().cloned().collect()
    }

    /// Bootstrap: pre-populate the registry with an empty actor for
    /// every distinct symbol we've seen events for, so that subsequent
    /// submits route correctly. Replay is the caller's responsibility.
    pub fn pre_populate(&self, symbols: impl IntoIterator<Item = Symbol>) {
        let mut guard = self.inner.lock().expect("actor registry mutex");
        for sym in symbols {
            guard.entry(sym.clone()).or_insert_with(|| {
                let (actor, _) = EngineActor::spawn(sym);
                actor
            });
        }
    }
}

impl Default for ActorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of `OrderService::submit_order`.
#[derive(Debug, Clone)]
pub struct SubmitResult {
    pub order_id: OrderId,
    pub fills: Vec<FillReceipt>,
    pub resting: bool,
    pub cancelled_remainder: u64,
}

/// One fill emitted during settlement.
#[derive(Debug, Clone)]
pub struct FillReceipt {
    pub maker_order_id: OrderId,
    pub taker_order_id: OrderId,
    pub price: u64,
    pub qty: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("ledger: {0}")]
    Ledger(#[from] LedgerError),
    #[error("actor: {0}")]
    Actor(String),
    #[error("rate limited")]
    RateLimited,
    #[error("user not found: {0:?}")]
    UnknownUser(UserId),
    #[error("duplicate idempotency key")]
    DuplicateIdempotency,
}

/// Orchestrator: locks funds in the ledger, submits to the matching
/// engine actor, settles trades, and records to the WAL.
#[derive(Clone)]
pub struct OrderService {
    pub users: Arc<Mutex<crate::auth::InMemoryUserStore>>,
    pub ledger: Arc<Mutex<InMemoryLedger>>,
    pub actors: ActorRegistry,
    pub wal: Arc<Mutex<Wal>>,
    pub idempotency: Arc<IdempotencyCache>,
    pub rate_limit: Arc<RateLimiter>,
}

impl OrderService {
    pub fn new(wal_path: &Path) -> Result<Self, String> {
        let wal = Wal::open_or_create(wal_path).map_err(|e| e.to_string())?;
        Ok(OrderService {
            users: Arc::new(Mutex::new(crate::auth::InMemoryUserStore::new())),
            ledger: Arc::new(Mutex::new(InMemoryLedger::new())),
            actors: ActorRegistry::new(),
            wal: Arc::new(Mutex::new(wal)),
            idempotency: Arc::new(IdempotencyCache::new(300)),
            rate_limit: Arc::new(RateLimiter::new(BucketConfig {
                capacity: 50,
                refill_per_sec: 10,
            })),
        })
    }

    /// Bootstrap: read all WAL events, replay each into a fresh ledger
    /// + matching-engine actor, leave the registry pre-populated.
    /// Returns the number of events replayed.
    pub fn bootstrap_from_wal(&self, wal_path: &Path) -> Result<usize, String> {
        // Wipe in-memory state so replay is deterministic.
        {
            let mut ledger = self.ledger.lock().unwrap();
            *ledger = InMemoryLedger::new();
        }
        // Drop the pre-existing users state and reseed; for Phase 5 we
        // assume the user store is populated separately (admin / boot
        // script). Replay only reconstructs the ledger + matching
        // engine book.
        let events = Wal::replay_all(wal_path).map_err(|e| e.to_string())?;
        // Pre-populate the actor registry with every symbol we've
        // seen.
        let symbols: Vec<Symbol> = events.iter().map(|e| Symbol::from(e.symbol.as_str())).collect();
        self.actors.pre_populate(symbols);

        // Apply each event.
        for event in events {
            self.apply_event(&event)?;
        }
        Ok(self.wal.lock().unwrap().next_seq() as usize - 1)
    }

    /// Apply a single WAL event to the matching engine. Used by
    /// `bootstrap_from_wal`.
    ///
    /// **Phase 5 scope:** replay rebuilds the matching-engine book
    /// only. The ledger is treated as a per-process cache; admin
    /// credits are not in the WAL and so a replayed process has
    /// empty ledger balances. Recording all ledger mutations in the
    /// WAL is a Phase 6+ widening.
    pub fn apply_event(&self, event: &WalEvent) -> Result<(), String> {
        let symbol = Symbol::from(event.symbol.as_str());
        match event.action {
            WalAction::Submit => {
                let side = if event.side == 0 { Side::Buy } else { Side::Sell };
                let kind = if event.order_kind == 0 { OrderKind::Limit } else { OrderKind::Market };
                let order = Order {
                    id: OrderId(event.order_id),
                    side,
                    price: event.price.map(Price),
                    qty: Qty(event.qty),
                    timestamp: Timestamp(event.timestamp),
                    kind,
                };
                let actor = self.actors.get_or_create(symbol);
                // Block-on since apply_event is sync; replay only
                // happens at startup so this is acceptable.
                futures::executor::block_on(actor.submit_limit(order))
                    .map_err(|e| format!("actor: {}", e))?;
            }
            WalAction::Cancel => {
                let actor = self.actors.get_or_create(symbol);
                let _ = futures::executor::block_on(actor.cancel(OrderId(event.order_id)))
                    .map_err(|e| format!("actor cancel: {}", e))?;
            }
        }
        Ok(())
    }

    /// Submit an order. Live path: rate-limit → ledger.place() → actor
    /// submit → for each trade, ledger.settle_trade() → WAL append.
    pub async fn submit_order(
        &self,
        user_id: UserId,
        symbol: Symbol,
        side: Side,
        kind: OrderKind,
        price: Option<Price>,
        qty: Qty,
        timestamp: Timestamp,
    ) -> Result<SubmitResult, ServiceError> {
        // 1. Rate limit.
        if !self.rate_limit.try_take(user_id.0) {
            return Err(ServiceError::RateLimited);
        }

        // 2. Build canonical Order + PlaceOrder.
        let order_id = OrderId(next_order_id());
        let order = Order {
            id: order_id,
            side,
            price,
            qty,
            timestamp,
            kind,
        };
        let place = PlaceOrder {
            id: order_id,
            user: user_id,
            symbol: symbol.clone(),
            side,
            kind,
            price,
            qty,
            timestamp,
        };

        // 3. Lock funds.
        {
            let mut ledger = self.ledger.lock().unwrap();
            ledger.place(&place)?;
        }

        // 4. Submit to actor.
        let actor = self.actors.get_or_create(symbol.clone());
        let match_result = actor
            .submit_limit(order)
            .await
            .map_err(|e| ServiceError::Actor(e.to_string()))?;

        // 5. Settle trades on ledger.
        let fills: Vec<FillReceipt> = {
            let mut ledger = self.ledger.lock().unwrap();
            let mut fills = Vec::with_capacity(match_result.trades.len());
            for trade in &match_result.trades {
                let settlement = TradeSettlement {
                    symbol: symbol.clone(),
                    maker_order_id: trade.maker_order_id,
                    taker_order_id: trade.taker_order_id,
                    price: trade.price,
                    qty: trade.qty,
                    taker_side: side,
                };
                ledger.settle_trade(&settlement)?;
                fills.push(FillReceipt {
                    maker_order_id: trade.maker_order_id,
                    taker_order_id: trade.taker_order_id,
                    price: trade.price.0,
                    qty: trade.qty.0,
                });
            }
            fills
        };

        // 6. Append to WAL.
        {
            let mut wal = self.wal.lock().unwrap();
            wal.append(WalEvent {
                seq: 0,
                action: WalAction::Submit,
                order_id: order_id.0,
                user_id: user_id.0,
                symbol: symbol.as_str().to_owned(),
                side: match side {
                    Side::Buy => 0,
                    Side::Sell => 1,
                },
                order_kind: match kind {
                    OrderKind::Limit => 0,
                    OrderKind::Market => 1,
                },
                price: price.map(|p| p.0),
                qty: qty.0,
                timestamp: timestamp.0,
            })
            .map_err(|e| ServiceError::Actor(format!("wal: {}", e)))?;
        }

        Ok(SubmitResult {
            order_id,
            fills,
            resting: match_result.resting_order_id.is_some(),
            cancelled_remainder: match_result.cancelled_remainder_qty.0,
        })
    }
}

/// Monotonic order id generator. Process-local; collisions across
/// processes are avoided by the WAL's monotonic seq.
fn next_order_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

// Re-export PlaceReceipt so older imports keep working.
pub use ledger::PlaceReceipt as _LedgerPlaceReceipt;
pub type LedgerPlaceReceipt = PlaceReceipt;