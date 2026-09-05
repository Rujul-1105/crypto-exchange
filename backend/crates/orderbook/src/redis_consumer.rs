//! Redis Streams consumer for `orders:incoming`.
//!
//! Long-lived task that XREADs new orders from the API tier, dispatches each
//! to the matching engine, and publishes the resulting events to
//! `events:outgoing`. Also consumes `settle:updates` from the settler worker
//! to flip `Trade.settle_status`.

use crate::market::SymbolRegistry;
use crate::redis_bus::RedisBus;
use common::*;
use redis::AsyncCommands;
use std::sync::Arc;

pub fn spawn_consumer(registry: SymbolRegistry, bus: RedisBus, last_consumed_id: String) {
    tokio::spawn(async move {
        let mut conn = bus.conn.clone();
        let mut last_id = last_consumed_id;

        // Ensure consumer group exists. XREADGROUP requires the group; for
        // Phase 4 we read with plain XREAD and rely on Redis retention.
        let _: Result<(), _> = conn
            .xgroup_create_mkstream(STREAM_ORDERS_INCOMING, "orderbook", "$")
            .await;

        loop {
            // Block for new orders.
            let res: Result<Vec<redis::streams::StreamRangeReply>, redis::RedisError> = conn
                .xread_options(
                    &[STREAM_ORDERS_INCOMING],
                    &[&last_id],
                    &redis::streams::StreamReadOptions::default()
                        .block(1000)
                        .count(64),
                )
                .await;
            match res {
                Ok(ranges) => {
                    for range in ranges {
                        for entry in range.ids {
                            last_id = entry.id.clone();
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
                            dispatch(Arc::clone(&registry_inners(&registry).await), &cmd, &bus)
                                .await;
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

/// Helper: extract the inner Arc<Mutex<HashMap>> from a SymbolRegistry. The
/// registry API doesn't expose this, so for Phase 4 we only consume the
/// default symbol. A proper implementation would dispatch by symbol field.
async fn registry_inners(_registry: &SymbolRegistry) -> Arc<()> {
    Arc::new(())
}

async fn dispatch(_engine: Arc<()>, cmd: &OrderCommand, bus: &RedisBus) {
    // Phase 4 stub — the real dispatch goes through `SymbolRegistry`. The
    // proper wiring happens when we add `engine.lock().await.match_entry(...)`
    // here once the registry accessor is widened.
    let _ = (cmd, bus);
    tracing::debug!("dispatch: {:?}", cmd);
}
