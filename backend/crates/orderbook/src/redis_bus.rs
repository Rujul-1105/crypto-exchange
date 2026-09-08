//! Redis Streams bridge — consumes `orders:incoming`, publishes to
//! `events:outgoing`, and persists durable snapshots.
//!
//! Phase 4 lays down the consumer + producer plumbing; snapshot/replay is
//! refined in `snapshot.rs`.
//!
//! `XTRIM MAXLEN ~` is called on every publish so disk usage stays bounded
//! even if no manual GC runs. The mirror cursor (`event_cursor`) is updated
//! after each `XADD events:outgoing` so the snapshot task can persist the
//! live position — on restart, an API WS client that reconnects with
//! `Last-Event-Id` replays missed events instead of seeing a 10-second gap.

use common::*;
use redis::{aio::ConnectionManager, AsyncCommands};
use std::sync::{Arc, Mutex};

/// Shared handle to the last-published `events:outgoing` stream id. The
/// snapshot task reads this so a restart picks up exactly where publishing
/// left off.
pub type EventCursor = Arc<Mutex<String>>;

pub fn new_event_cursor(initial: impl Into<String>) -> EventCursor {
    Arc::new(Mutex::new(initial.into()))
}

/// Default cap on `events:outgoing` length. With ~200-byte envelopes and
/// 50 events/sec at peak, 100k entries ≈ 30 minutes of buffer before
/// trimming.
pub const DEFAULT_EVENTS_MAXLEN: usize = 100_000;
/// Default cap on `settle:updates` length — much smaller, fills only on
/// every fill.
pub const DEFAULT_SETTLE_MAXLEN: usize = 10_000;

#[derive(Clone)]
pub struct RedisBus {
    pub conn: ConnectionManager,
    /// Live cursor mirrored into the snapshot so a restart resumes from
    /// the right point. Defaults to `"0-0"` (cold start).
    pub event_cursor: EventCursor,
    pub events_maxlen: usize,
    pub settle_maxlen: usize,
}

impl RedisBus {
    pub async fn connect(
        url: &str,
        event_cursor: EventCursor,
        events_maxlen: usize,
        settle_maxlen: usize,
    ) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self {
            conn,
            event_cursor,
            events_maxlen,
            settle_maxlen,
        })
    }

    /// Publish a list of events to `events:outgoing`. Returns the stream IDs.
    /// Updates the shared `event_cursor` with the last id assigned, and
    /// trims the stream with `XTRIM MAXLEN ~` so the AOF/RDB stays bounded.
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
            // Mirror the live cursor so the snapshot task persists it.
            if let Ok(mut c) = self.event_cursor.lock() {
                *c = id.clone();
            }
            ids.push(id);
        }
        // Best-effort trim — approximate MAXLEN (~) is O(1) amortized and
        // runs at idle moments, not on every XADD.
        let _: Result<i64, _> = conn
            .xtrim(STREAM_EVENTS_OUTGOING, redis::streams::StreamMaxlen::Approx(self.events_maxlen))
            .await;
        Ok(ids)
    }

    /// Publish a settlement update on `settle:updates`. Trims the stream.
    pub async fn publish_settle_update(&self, u: &SettleUpdate) -> anyhow::Result<String> {
        let mut conn = self.conn.clone();
        let payload = serde_json::to_string(u)?;
        let id: String = conn
            .xadd(STREAM_SETTLE_UPDATES, "*", &[("data", payload.as_str())])
            .await?;
        let _: Result<i64, _> = conn
            .xtrim(STREAM_SETTLE_UPDATES, redis::streams::StreamMaxlen::Approx(self.settle_maxlen))
            .await;
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
        // Redis 0.27's XREAD parses into `StreamReadReply { keys }`.
        let res: redis::streams::StreamReadReply = conn
            .xread_options(
                &[STREAM_SETTLE_UPDATES],
                &[last_id],
                &redis::streams::StreamReadOptions::default()
                    .block(block_ms)
                    .count(64),
            )
            .await?;
        let mut out = Vec::new();
        for stream_key in res.keys {
            for entry in stream_key.ids {
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