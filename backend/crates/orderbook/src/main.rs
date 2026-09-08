//! Orderbook server entrypoint — wires actix-web admin, the symbol registry,
//! the Redis Streams consumer, the snapshot task, and the market-maker bot.

use actix_cors::Cors;
use actix_web::{middleware, web, App, HttpServer};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

use common::DEFAULT_SYMBOL;
use orderbook::{
    admin::{self, AdminState},
    bot::{spawn_bot, BotConfig},
    market::SymbolRegistry,
    redis_bus::RedisBus,
    redis_consumer::{spawn_consumer, spawn_settle_consumer},
    snapshot::{hydrate_registry, load_latest_snapshot, new_cursor, spawn_snapshot_task},
};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,orderbook=debug")),
        )
        .init();

    // ── Config from env ──
    let bind = std::env::var("ORDERBOOK_BIND").unwrap_or_else(|_| "127.0.0.1:8081".into());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
    let snapshot_dir: PathBuf = std::env::var("ORDERBOOK_SNAPSHOT_DIR")
        .unwrap_or_else(|_| "./snapshots".into())
        .into();
    let snapshot_interval_ms: u64 = std::env::var("ORDERBOOK_SNAPSHOT_INTERVAL_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);
    let demo_symbols: Vec<String> = std::env::var("DEMO_SYMBOLS")
        .unwrap_or_else(|_| DEFAULT_SYMBOL.into())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // ── Redis bus ──
    let bus = RedisBus::connect(&redis_url)
        .await
        .map_err(|e| std::io::Error::other(format!("redis connect: {e}")))?;

    // ── Symbol registry + hydrate from latest snapshot if present ──
    let registry = SymbolRegistry::new();
    let initial_cursor = match load_latest_snapshot(&snapshot_dir).await {
        Ok(Some(state)) => {
            if let Err(e) = hydrate_registry(&registry, &state).await {
                tracing::warn!("hydrate from snapshot failed: {e}");
                "0".into()
            } else {
                tracing::info!(
                    "hydrated {} engine(s) from snapshot; resuming from cursor {}",
                    state.engines.len(),
                    state.last_consumed_order_redis_id
                );
                state.last_consumed_order_redis_id
            }
        }
        Ok(None) => {
            tracing::info!("no snapshot found, starting fresh");
            "0".into()
        }
        Err(e) => {
            tracing::warn!("snapshot load failed: {e}");
            "0".into()
        }
    };

    // Ensure each demo symbol has an engine.
    for sym in &demo_symbols {
        let _ = registry.get_or_create(sym.clone()).await;
    }

    // ── Shared cursor so the snapshot persists the live consumer position ──
    let cursor = new_cursor(initial_cursor.clone());

    // ── Spawn snapshot task ──
    spawn_snapshot_task(registry.clone(), snapshot_dir, snapshot_interval_ms, cursor.clone());

    // ── Spawn consumer (XREAD orders:incoming) ──
    spawn_consumer(registry.clone(), bus.clone(), initial_cursor, cursor.clone());

    // ── Spawn settle:updates consumer ──
    spawn_settle_consumer(registry.clone(), bus.clone(), "0".into());

    // ── Spawn market maker bot for the demo symbol ──
    for sym in &demo_symbols {
        let cfg = BotConfig::from_env(sym.clone());
        let engine = registry.get_or_create(sym.clone()).await;
        // The bot publishes its own events (and a noise taker's) through the
        // bus so WS subscribers see book + candle updates live.
        spawn_bot(engine, cfg, bus.clone());
    }

    // ── Start actix-web admin ──
    let state = AdminState {
        registry: registry.clone(),
    };

    let data = web::Data::new(state);
    tracing::info!("orderbook admin listening on {bind}");
    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header();

        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .app_data(data.clone())
            .route("/health", web::get().to(admin::health))
            .route("/api/symbols", web::get().to(admin::list_symbols))
            .route(
                "/api/orderbook/{symbol}",
                web::get().to(admin::orderbook_snapshot),
            )
            .route("/api/trades/{symbol}", web::get().to(admin::recent_trades))
            .route(
                "/api/candles/{symbol}/{interval}",
                web::get().to(admin::candles),
            )
    })
    .bind(&bind)?
    .run()
    .await
}
