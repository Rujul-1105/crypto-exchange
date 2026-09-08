//! Market maker bot — spawns one quoter per symbol, quotes N levels each
//! side around the last trade price, and publishes all resulting events
//! through the Redis bus so WS subscribers see live book + candle updates.
//!
//! A second concurrent task fires a small noise taker (market order) at a
//! slower cadence that crosses the bot's own spread, producing real trades
//! so the candle aggregator, trade tape, and any future 24h stats see
//! activity. With only the quoter and no taker, `last_trade_price` would
//! stay pinned at the configured starting mid forever.

use common::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;

use crate::engine::MatchingEngine;
use crate::redis_bus::RedisBus;

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

pub fn spawn_bot(engine: Arc<Mutex<MatchingEngine>>, config: BotConfig, bus: RedisBus) {
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

    // Quoter task — cancel-replace N levels each side.
    let engine_q = engine.clone();
    let config_q = config.clone();
    let bus_q = bus.clone();
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_millis(config_q.quote_interval_ms));
        let mut prev_buy_ids: Vec<OrderId> = Vec::new();
        let mut prev_sell_ids: Vec<OrderId> = Vec::new();
        loop {
            ticker.tick().await;
            let mid = {
                let eng = engine_q.lock().await;
                eng.book.last_trade_price.unwrap_or(config_q.starting_mid)
            };
            let step = mid * Price::from(config_q.level_step_bps) / Price::from(10_000);
            let half_spread = mid * Price::from(config_q.spread_bps) / Price::from(20_000);
            let bid_top = mid - half_spread;
            let ask_top = mid + half_spread;
            let user = format!("bot:{}", config_q.symbol);
            let now = chrono::Utc::now().timestamp_millis();

            // Cancel previous quotes; collect + publish the resulting events
            // so WS book deltas reflect the cancels.
            let cancel_events: Vec<EngineEvent> = {
                let mut eng = engine_q.lock().await;
                let mut evs = Vec::new();
                for id in prev_buy_ids.drain(..) {
                    evs.extend(eng.cancel(id, now));
                }
                for id in prev_sell_ids.drain(..) {
                    evs.extend(eng.cancel(id, now));
                }
                evs
            };
            if !cancel_events.is_empty() {
                let _ = bus_q.publish_events(&config_q.symbol, &cancel_events).await;
            }

            // Place new quotes.
            let mut all_events: Vec<EngineEvent> = Vec::new();
            let mut new_buy_ids = Vec::new();
            let mut new_sell_ids = Vec::new();
            for i in 0..config_q.num_levels {
                let bid_price = bid_top - step * Price::from(i as u64);
                let ask_price = ask_top + step * Price::from(i as u64);
                let buy = Order {
                    id: 0,
                    user: user.clone(),
                    symbol: config_q.symbol.clone(),
                    side: Side::Buy,
                    order_type: OrderType::Limit,
                    tif: TimeInForce::Gtc,
                    price: Some(bid_price),
                    quantity: config_q.level_size,
                    filled: Quantity::ZERO,
                    status: OrderStatus::New,
                    created_at: now,
                    updated_at: now,
                };
                let sell = Order {
                    id: 0,
                    user: user.clone(),
                    symbol: config_q.symbol.clone(),
                    side: Side::Sell,
                    order_type: OrderType::Limit,
                    tif: TimeInForce::Gtc,
                    price: Some(ask_price),
                    quantity: config_q.level_size,
                    filled: Quantity::ZERO,
                    status: OrderStatus::New,
                    created_at: now,
                    updated_at: now,
                };
                let mut eng = engine_q.lock().await;
                let buy_events = eng.match_entry(buy, now);
                if let Some(EngineEvent::Accepted { order }) = buy_events.last() {
                    new_buy_ids.push(order.id);
                }
                all_events.extend(buy_events);
                let sell_events = eng.match_entry(sell, now);
                if let Some(EngineEvent::Accepted { order }) = sell_events.last() {
                    new_sell_ids.push(order.id);
                }
                all_events.extend(sell_events);
            }
            if !all_events.is_empty() {
                let _ = bus_q.publish_events(&config_q.symbol, &all_events).await;
            }
            prev_buy_ids = new_buy_ids;
            prev_sell_ids = new_sell_ids;
        }
    });

    // Noise taker task — fires a small IOC market order at a slower cadence
    // (~3s) to cross the bot's spread. Without this, the only counter-party
    // is the bot itself and `last_trade_price` never moves. We skip every
    // 5th tick to break the deterministic pattern and alternate buy/sell so
    // the book doesn't drift one way.
    let engine_t = engine.clone();
    let config_t = config.clone();
    let bus_t = bus.clone();
    tokio::spawn(async move {
        // Wait for the first quoter tick to land before firing taker orders.
        tokio::time::sleep(Duration::from_millis(750)).await;
        let mut ticker = interval(Duration::from_millis(3_000));
        let mut tick_count: u64 = 0;
        loop {
            ticker.tick().await;
            tick_count = tick_count.wrapping_add(1);
            if tick_count % 5 == 0 {
                continue;
            }
            let user = format!("taker:noise:{}", config_t.symbol);
            let now = chrono::Utc::now().timestamp_millis();
            let side = if tick_count % 2 == 0 { Side::Buy } else { Side::Sell };
            let qty = Quantity::from_f64_retain(0.1).unwrap();
            let order = Order {
                id: 0,
                user,
                symbol: config_t.symbol.clone(),
                side,
                order_type: OrderType::Market,
                tif: TimeInForce::Ioc,
                price: None,
                quantity: qty,
                filled: Quantity::ZERO,
                status: OrderStatus::New,
                created_at: now,
                updated_at: now,
            };
            let events = {
                let mut eng = engine_t.lock().await;
                eng.match_entry(order, now)
            };
            if !events.is_empty() {
                let _ = bus_t.publish_events(&config_t.symbol, &events).await;
            }
        }
    });
}