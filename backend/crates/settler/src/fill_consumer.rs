//! Fill consumer — long-lived task that XREADs Fill events from
//! `events:outgoing`, dispatches each to the SettlerClient, and publishes the
//! result on `settle:updates`.

use common::*;
use redis::{aio::ConnectionManager, AsyncCommands};

use crate::anchor_client::SettlerClient;

pub struct FillConsumer {
    conn: ConnectionManager,
    settler: SettlerClient,
}

impl FillConsumer {
    pub fn new(conn: ConnectionManager, settler: SettlerClient) -> Self {
        Self { conn, settler }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let mut last_id = "0".to_string();
        loop {
            // Redis 0.27's XREAD parses into `StreamReadReply { keys }` —
            // the older `Vec<StreamRangeReply>` silently fails on every
            // response with "Response type not map compatible".
            let res: redis::streams::StreamReadReply = self
                .conn
                .clone()
                .xread_options(
                    &[STREAM_EVENTS_OUTGOING],
                    &[&last_id],
                    &redis::streams::StreamReadOptions::default()
                        .block(5000)
                        .count(64),
                )
                .await?;
            for stream_key in res.keys {
                for entry in stream_key.ids {
                    last_id = entry.id.clone();
                    let Some(payload) = entry.map.get("data") else { continue };
                    let Ok(s) = redis::from_redis_value::<String>(payload) else { continue };
                    let env: EventEnvelope = match serde_json::from_str(&s) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };
                    if let EngineEvent::Fill { trade, .. } = &env.event {
                        let symbol = trade.symbol.clone();
                        let update = self.settler.settle_fill(trade).await;
                        // Publish result to settle:updates so the orderbook server
                        // can flip Trade.settle_status and rebroadcast.
                        let payload = serde_json::to_string(&update)?;
                        let _: String = self
                            .conn
                            .clone()
                            .xadd(STREAM_SETTLE_UPDATES, "*", &[("data", payload.as_str())])
                            .await?;
                        tracing::info!(
                            "settled trade_id={} status={:?} sig={:?} symbol={}",
                            update.trade_id,
                            update.status,
                            update.signature,
                            symbol
                        );
                    }
                }
            }
        }
    }
}
