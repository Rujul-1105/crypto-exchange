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
}
