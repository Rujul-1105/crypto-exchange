//! Redis Streams bridge — consumes `orders:incoming`, publishes to
//! `events:outgoing`, and persists durable snapshots.
//!
//! Phase 4 lays down the consumer + producer plumbing; snapshot/replay is
//! refined in `snapshot.rs`.

use common::*;
use redis::{aio::ConnectionManager, AsyncCommands};

#[derive(Clone)]
pub struct RedisBus {
    pub conn: ConnectionManager,
}

impl RedisBus {
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    /// Publish a list of events to `events:outgoing`. Returns the stream IDs.
    pub async fn publish_events(
        &self,
        symbol: &Symbol,
        events: &[EngineEvent],
    ) -> anyhow::Result<Vec<String>> {
        let mut conn = self.conn.clone();
        let mut ids = Vec::with_capacity(events.len());
        for ev in events {
            let env = EventEnvelope::new("*", symbol.clone(), ev.clone());
            let payload = serde_json::to_string(&env)?;
            // XADD <stream> * data <json>
            let id: String = conn
                .xadd(STREAM_EVENTS_OUTGOING, "*", &[("data", payload.as_str())])
                .await?;
            ids.push(id);
        }
        Ok(ids)
    }

    /// Publish a settlement update on `settle:updates`.
    pub async fn publish_settle_update(&self, u: &SettleUpdate) -> anyhow::Result<String> {
        let mut conn = self.conn.clone();
        let payload = serde_json::to_string(u)?;
        let id: String = conn
            .xadd(STREAM_SETTLE_UPDATES, "*", &[("data", payload.as_str())])
            .await?;
        Ok(id)
    }

    /// Read settlement updates from `settle:updates` with `BLOCK block_ms`.
    /// Returns `(stream_id, SettleUpdate)` pairs.
    pub async fn read_settle_updates(
        &self,
        last_id: &str,
        block_ms: usize,
    ) -> anyhow::Result<Vec<(String, SettleUpdate)>> {
        let mut conn = self.conn.clone();
        let res: Vec<redis::streams::StreamRangeReply> = conn
            .xread_options(
                &[STREAM_SETTLE_UPDATES],
                &[last_id],
                &redis::streams::StreamReadOptions::default()
                    .block(block_ms)
                    .count(64),
            )
            .await?;
        let mut out = Vec::new();
        for range in res {
            for entry in range.ids {
                let Some(payload) = entry.map.get("data") else {
                    continue;
                };
                let Ok(s) = redis::from_redis_value::<String>(payload) else {
                    tracing::warn!("non-string payload in settle:updates");
                    continue;
                };
                match serde_json::from_str::<SettleUpdate>(&s) {
                    Ok(u) => out.push((entry.id, u)),
                    Err(e) => tracing::warn!("failed to parse SettleUpdate: {e}"),
                }
            }
        }
        Ok(out)
    }
}
