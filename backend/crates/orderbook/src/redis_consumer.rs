//! Redis Streams consumer for `orders:incoming`.
//!
//! Long-lived task that XREADs new orders from the API tier, dispatches each
//! to the matching engine, and publishes the resulting events to
//! `events:outgoing`.
//!
//! The consumer uses plain `XREAD` with a tracked `last_id`. Consumer-group
//! semantics (`XREADGROUP`) would only earn their keep with multiple replicas
//! sharing work — this orderbook is single-replica, so plain XREAD is
//! sufficient.

use crate::market::SymbolRegistry;
use crate::redis_bus::RedisBus;
use crate::snapshot::Cursor;
use common::*;
use redis::AsyncCommands;

pub fn spawn_consumer(
    registry: SymbolRegistry,
    bus: RedisBus,
    last_consumed_id: String,
    cursor: Cursor,
) {
    tokio::spawn(async move {
        let mut conn = bus.conn.clone();
        let mut last_id = last_consumed_id;

        loop {
            // Block for new orders. Redis 0.27's XREAD parses into
            // `StreamReadReply { keys }` — `Vec<StreamRangeReply>` silently
            // fails on every response with "Response type not map compatible".
            let res: Result<redis::streams::StreamReadReply, redis::RedisError> = conn
                .xread_options(
                    &[STREAM_ORDERS_INCOMING],
                    &[&last_id],
                    &redis::streams::StreamReadOptions::default()
                        .block(1000)
                        .count(64),
                )
                .await;
            match res {
                Ok(reply) => {
                    for stream_key in reply.keys {
                        for entry in stream_key.ids {
                            last_id = entry.id.clone();
                            // Mirror the cursor into the shared handle so the
                            // snapshot task persists the live position.
                            if let Ok(mut c) = cursor.lock() {
                                *c = last_id.clone();
                            }
                            let Some(payload) = entry.map.get("data") else {
                                continue;
                            };
                            let Ok(payload_str) = redis::from_redis_value::<String>(payload) else {
                                tracing::warn!("non-string payload in orders:incoming");
                                continue;
                            };
                            let cmd: OrderCommand = match serde_json::from_str(&payload_str) {
                                Ok(c) => c,
                                Err(e) => {
                                    tracing::warn!("failed to parse OrderCommand: {e}");
                                    continue;
                                }
                            };
                            dispatch(&registry, cmd, &bus).await;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("xread error: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    });
}

/// Apply a parsed `OrderCommand` to the matching engine and publish the
/// resulting events back out on `events:outgoing`. A command targeting an
/// unknown symbol is logged and dropped (no engine, no fill).
async fn dispatch(registry: &SymbolRegistry, cmd: OrderCommand, bus: &RedisBus) {
    let symbol = cmd.symbol().clone();
    let now = chrono::Utc::now().timestamp_millis();

    let Some(engine) = registry.get(&symbol).await else {
        tracing::warn!("dispatch: no engine for symbol={symbol}");
        return;
    };

    let events = match cmd {
        OrderCommand::Place(place) => {
            // Construct the engine-internal `Order`. The engine assigns the
            // final `id` from its `OrderIdGen` — pass 0 as a placeholder.
            let order = Order {
                id: 0,
                user: place.user,
                symbol: place.symbol,
                side: place.side,
                order_type: place.order_type,
                tif: place.tif,
                price: place.price,
                quantity: place.quantity,
                filled: Quantity::ZERO,
                status: OrderStatus::New,
                created_at: now,
                updated_at: now,
            };
            let mut eng = engine.lock().await;
            eng.match_entry(order, now)
        }
        OrderCommand::Cancel(cancel) => {
            let mut eng = engine.lock().await;
            eng.cancel(cancel.order_id, now)
        }
        OrderCommand::Amend(amend) => {
            let mut eng = engine.lock().await;
            eng.amend(amend.order_id, amend.new_quantity, amend.new_price, now)
        }
    };

    if events.is_empty() {
        return;
    }
    if let Err(e) = bus.publish_events(&symbol, &events).await {
        tracing::warn!("publish_events failed for {symbol}: {e}");
    }
}

/// Consume `settle:updates` and flip `Trade.settle_status` in each registered
/// engine. Each successfully-flipped trade is rebroadcast as an
/// `EngineEvent::SettleUpdate` on `events:outgoing`.
pub fn spawn_settle_consumer(registry: SymbolRegistry, bus: RedisBus, last_id: String) {
    tokio::spawn(async move {
        let mut last = last_id;
        loop {
            match bus.read_settle_updates(&last, 1000).await {
                Ok(updates) => {
                    for (stream_id, update) in updates {
                        last = stream_id;
                        // Walk every engine; the trade might be in any of
                        // them (single-symbol today, but cheap to generalize).
                        let symbols = registry.list().await;
                        for sym in symbols {
                            let Some(engine) = registry.get(&sym).await else {
                                continue;
                            };
                            let rebroadcast = {
                                let mut eng = engine.lock().await;
                                eng.update_settle_status(update.trade_id, update.status)
                                    .map(|trade| (sym.clone(), trade))
                            };
                            if let Some((sym, trade)) = rebroadcast {
                                let ev = EngineEvent::SettleUpdate {
                                    trade_id: trade.id,
                                    symbol: sym.clone(),
                                    status: trade.settle_status,
                                    signature: update.signature.clone(),
                                    error: update.error.clone(),
                                };
                                if let Err(e) = bus.publish_events(&sym, &[ev]).await {
                                    tracing::warn!(
                                        "publish SettleUpdate failed for {sym}: {e}"
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("read_settle_updates error: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    });
}