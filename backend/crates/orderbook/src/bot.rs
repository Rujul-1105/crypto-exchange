//! Market maker bot — spawns one quoter per symbol, quotes N levels each side
//! around the last trade price (or a configured starting mid).

use common::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;

use crate::engine::MatchingEngine;

#[derive(Debug, Clone)]
pub struct BotConfig {
    pub enabled: bool,
    pub symbol: Symbol,
    pub starting_mid: Price,
    pub spread_bps: u16,
    pub level_step_bps: u16,
    pub level_size: Quantity,
    pub num_levels: usize,
    pub quote_interval_ms: u64,
}

impl BotConfig {
    pub fn from_env(symbol: Symbol) -> Self {
        let starting_mid: f64 = std::env::var(format!("{}_STARTING_MID", symbol.replace('-', "_")))
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(150.0);
        Self {
            enabled: std::env::var("DEMO_BOT").ok().as_deref() != Some("false"),
            symbol,
            starting_mid: Price::from_f64_retain(starting_mid).unwrap(),
            spread_bps: std::env::var("DEMO_BOT_SPREAD_BPS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            level_step_bps: std::env::var("DEMO_BOT_LEVEL_STEP_BPS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
            level_size: std::env::var("DEMO_BOT_LEVEL_SIZE")
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .and_then(Quantity::from_f64_retain)
                .unwrap_or_else(|| Quantity::from_f64_retain(0.5).unwrap()),
            num_levels: 10,
            quote_interval_ms: std::env::var("DEMO_BOT_QUOTE_INTERVAL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500),
        }
    }
}

pub fn spawn_bot(engine: Arc<Mutex<MatchingEngine>>, config: BotConfig) {
    if !config.enabled {
        tracing::info!("market maker bot disabled for {}", config.symbol);
        return;
    }
    tracing::info!(
        "spawning market maker for {} (spread={}bps step={}bps size={} levels={})",
        config.symbol,
        config.spread_bps,
        config.level_step_bps,
        config.level_size,
        config.num_levels
    );
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_millis(config.quote_interval_ms));
        // Track the bot's previous quote ids so we can cancel-then-replace.
        let mut prev_buy_ids: Vec<OrderId> = Vec::new();
        let mut prev_sell_ids: Vec<OrderId> = Vec::new();
        loop {
            ticker.tick().await;
            // Compute mid price (use last trade if we have one, else starting_mid).
            let mid = {
                let eng = engine.lock().await;
                eng.book.last_trade_price.unwrap_or(config.starting_mid)
            };
            // Compute N levels each side.
            let step = mid * Price::from(config.level_step_bps) / Price::from(10_000);
            let half_spread = mid * Price::from(config.spread_bps) / Price::from(20_000);
            let bid_top = mid - half_spread;
            let ask_top = mid + half_spread;
            let user = format!("bot:{}", config.symbol);
            let now = chrono::Utc::now().timestamp_millis();

            // Cancel previous quotes.
            {
                let mut eng = engine.lock().await;
                for id in prev_buy_ids.drain(..) {
                    eng.cancel(id, now);
                }
                for id in prev_sell_ids.drain(..) {
                    eng.cancel(id, now);
                }
            }

            // Place new quotes.
            let mut new_buy_ids = Vec::new();
            let mut new_sell_ids = Vec::new();
            for i in 0..config.num_levels {
                let bid_price = bid_top - step * Price::from(i as u64);
                let ask_price = ask_top + step * Price::from(i as u64);
                let buy = Order {
                    id: 0,
                    user: user.clone(),
                    symbol: config.symbol.clone(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    tif: TimeInForce::Gtc,
                    price: Some(bid_price),
                    quantity: config.level_size,
                    filled: Quantity::ZERO,
                    status: OrderStatus::New,
                    created_at: now,
                    updated_at: now,
                };
                let sell = Order {
                    id: 0,
                    user: user.clone(),
                    symbol: config.symbol.clone(),
                    side: Side::Sell,
                    order_type: OrderType::Limit,
                    tif: TimeInForce::Gtc,
                    price: Some(ask_price),
                    quantity: config.level_size,
                    filled: Quantity::ZERO,
                    status: OrderStatus::New,
                    created_at: now,
                    updated_at: now,
                };
                let mut eng = engine.lock().await;
                let buy_events = eng.match_entry(buy, now);
                if let Some(EngineEvent::Accepted { order }) = buy_events.last() {
                    new_buy_ids.push(order.id);
                }
                let sell_events = eng.match_entry(sell, now);
                if let Some(EngineEvent::Accepted { order }) = sell_events.last() {
                    new_sell_ids.push(order.id);
                }
            }
            prev_buy_ids = new_buy_ids;
            prev_sell_ids = new_sell_ids;
        }
    });
}
