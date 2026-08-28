//! Redis Streams bridge — publish orders to `orders:incoming`.

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

    pub async fn push_order(&self, cmd: &OrderCommand) -> anyhow::Result<String> {
        let mut conn = self.conn.clone();
        let payload = serde_json::to_string(cmd)?;
        let id: String = conn
            .xadd(STREAM_ORDERS_INCOMING, "*", &[("data", payload.as_str())])
            .await?;
        Ok(id)
    }

    /// XREAD from `events:outgoing` starting at `last_id`.
    pub async fn read_events(
        &self,
        last_id: &str,
        block_ms: usize,
    ) -> anyhow::Result<Vec<(String, EventEnvelope)>> {
        use futures::stream::TryStreamExt;
        let mut conn = self.conn.clone();
        let opts = redis::streams::StreamReadOptions::default()
            .block(block_ms)
            .count(256);
        let res: Vec<redis::streams::StreamRangeReply> = conn
            .xread_options(&[STREAM_EVENTS_OUTGOING], &[last_id], &opts)
            .await?;
        let mut out = Vec::new();
        for range in res {
            for entry in range.ids {
                let Some(payload) = entry.map.get("data") else { continue };
                let Ok(payload_str) = redis::from_redis_value::<String>(payload) else {
                    continue;
                };
                if let Ok(env) = serde_json::from_str::<EventEnvelope>(&payload_str) {
                    out.push((entry.id, env));
                }
            }
        }
        Ok(out)
    }
}
