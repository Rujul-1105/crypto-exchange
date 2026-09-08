//! Redis Streams bridge — publish orders to `orders:incoming`.

use common::*;
use redis::{aio::ConnectionManager, AsyncCommands};

/// Default cap on `orders:incoming` length. With ~200-byte commands and
/// peak ~10 orders/sec/user, 100k entries ≈ 2.5 hours of buffer.
pub const DEFAULT_ORDERS_MAXLEN: usize = 100_000;

#[derive(Clone)]
pub struct RedisBus {
    pub conn: ConnectionManager,
    pub orders_maxlen: usize,
}

impl RedisBus {
    pub async fn connect(url: &str, orders_maxlen: usize) -> anyhow::Result<Self> {
        let client = redis::Client::open(url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn, orders_maxlen })
    }

    pub async fn push_order(&self, cmd: &OrderCommand) -> anyhow::Result<String> {
        let mut conn = self.conn.clone();
        let payload = serde_json::to_string(cmd)?;
        // XADD <stream> * data <json>
        let id: String = conn
            .xadd(STREAM_ORDERS_INCOMING, "*", &[("data", payload.as_str())])
            .await?;
        // Trim the stream so disk usage stays bounded. Approximate MAXLEN (~)
        // is O(1) amortized and trims at idle moments, not on every XADD.
        let _: Result<i64, _> = conn
            .xtrim(
                STREAM_ORDERS_INCOMING,
                redis::streams::StreamMaxlen::Approx(self.orders_maxlen),
            )
            .await;
        Ok(id)
    }

    /// XREAD from `events:outgoing` starting at `last_id`.
    pub async fn read_events(
        &self,
        last_id: &str,
        block_ms: usize,
    ) -> anyhow::Result<Vec<(String, EventEnvelope)>> {
        let mut conn = self.conn.clone();
        let opts = redis::streams::StreamReadOptions::default()
            .block(block_ms)
            .count(256);
        // Redis 0.27's XREAD parses into `StreamReadReply { keys: Vec<StreamKey> }`
        // — the older `Vec<StreamRangeReply>` shape silently fails on every
        // response with "Response type not map compatible".
        let res: redis::streams::StreamReadReply = conn
            .xread_options(&[STREAM_EVENTS_OUTGOING], &[last_id], &opts)
            .await?;
        let mut out = Vec::new();
        for stream_key in res.keys {
            for entry in stream_key.ids {
                let Some(payload) = entry.map.get("data") else {
                    continue;
                };
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

    /// Store an SIWS nonce with a TTL (seconds). Single-write; overwrites any
    /// existing nonce for the pubkey.
    pub async fn put_nonce(
        &self,
        pubkey: &str,
        nonce: &str,
        ttl_secs: u64,
    ) -> anyhow::Result<()> {
        let mut conn = self.conn.clone();
        let key = format!("nonce:{pubkey}");
        let _: () = conn.set_ex(&key, nonce, ttl_secs).await?;
        Ok(())
    }

    /// Atomically read+delete the nonce for `pubkey`. The GETDEL makes
    /// replay impossible — a second verify after the first consumes the
    /// nonce. Returns `Ok(None)` if absent.
    pub async fn take_nonce(&self, pubkey: &str) -> anyhow::Result<Option<String>> {
        let mut conn = self.conn.clone();
        let key = format!("nonce:{pubkey}");
        let val: Option<String> = conn.get_del(&key).await?;
        Ok(val)
    }
}
