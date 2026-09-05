//! API server (actix-web) — stateless REST + WebSocket. Bridges clients to
//! the orderbook server via Redis Streams and an internal HTTP proxy to the
//! orderbook's admin endpoints for reads.

use actix_cors::Cors;
use actix_web::{middleware, web, App, HttpServer};
use common::{EngineEvent, EventEnvelope, OrderStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

mod auth;
mod redis_bus;
mod routes;
mod ws;

use redis_bus::RedisBus;
use routes::UserOrdersIndex;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,api=debug")),
        )
        .init();

    let bind = std::env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
    let orderbook_admin = std::env::var("ORDERBOOK_ADMIN_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8081".into());
    let allowed_origins: Vec<String> = std::env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000".into())
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "dev-only-secret-change-me-in-production".into());

    let bus = RedisBus::connect(&redis_url)
        .await
        .map_err(|e| std::io::Error::other(format!("redis: {e}")))?;
    let user_orders: UserOrdersIndex = Arc::new(RwLock::new(HashMap::new()));
    let state = routes::ApiState {
        bus: bus.clone(),
        orderbook_admin,
        allowed_origins: allowed_origins.clone(),
        jwt_secret: jwt_secret.clone(),
        nonce_store: crate::auth::NonceStore::new(bus.clone()),
        user_orders: user_orders.clone(),
    };
    let data = web::Data::new(state);

    // Spawn events:outgoing tracker — keeps the user→order_id→symbol index
    // fresh so cancel/amend can resolve the symbol.
    spawn_user_orders_tracker(bus.clone(), user_orders);

    tracing::info!("api server listening on {bind}");
    HttpServer::new(move || {
        // CORS: allow only origins in `ALLOWED_ORIGINS` (comma-separated env
        // var). Default = `http://localhost:3000` so the Next.js dev server
        // works out of the box.
        let mut cors = Cors::default()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);
        for origin in &allowed_origins {
            cors = cors.allowed_origin(origin);
        }
        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::default())
            .app_data(data.clone())
            .route("/health", web::get().to(routes::health))
            .service(
                web::scope("/api")
                    .route("/symbols", web::get().to(routes::symbols))
                    .route("/orderbook/{symbol}", web::get().to(routes::orderbook))
                    .route("/trades/{symbol}", web::get().to(routes::trades))
                    .route(
                        "/candles/{symbol}/{interval}",
                        web::get().to(routes::candles),
                    )
                    .route("/auth/nonce", web::post().to(routes::auth_nonce))
                    .route("/auth/verify", web::post().to(routes::auth_verify))
                    .route("/orders", web::post().to(routes::place_order))
                    .route("/orders", web::get().to(routes::list_orders))
                    .route("/orders/{id}", web::get().to(routes::get_order))
                    .route("/orders/{id}", web::delete().to(routes::cancel_order))
                    .route("/orders/{id}", web::put().to(routes::amend_order))
                    .route("/balances", web::get().to(routes::balances)),
            )
            .route("/ws", web::get().to(ws::ws_handler))
    })
    .bind(&bind)?
    .run()
    .await
}

/// Long-lived task that XREADs `events:outgoing` and updates the user-orders
/// index. Inserts on `Accepted` (open status only), removes on `Cancelled` /
/// `Rejected` / `Filled` (status terminal).
fn spawn_user_orders_tracker(bus: RedisBus, user_orders: UserOrdersIndex) {
    tokio::spawn(async move {
        let mut last_id = "0".to_string();
        loop {
            match bus.read_events(&last_id, 1000).await {
                Ok(events) => {
                    for (stream_id, env) in events {
                        last_id = stream_id;
                        if let Some((user, order_id, symbol, open)) =
                            order_index_update(&env.event)
                        {
                            let mut map = user_orders.write().await;
                            let entry = map.entry(user).or_default();
                            if open {
                                entry.insert(order_id, symbol);
                            } else {
                                entry.remove(&order_id);
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("user_orders_tracker: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    });
}

/// Extract `(user, order_id, symbol, open)` from an event. Returns `None`
/// for events that don't change the index.
fn order_index_update(
    event: &EngineEvent,
) -> Option<(String, u64, String, bool)> {
    match event {
        EngineEvent::Accepted { order } => {
            let open = matches!(
                order.status,
                OrderStatus::New | OrderStatus::PartiallyFilled
            );
            Some((order.user.clone(), order.id, order.symbol.clone(), open))
        }
        EngineEvent::Cancelled { id, user, symbol, .. } => {
            Some((user.clone(), *id, symbol.clone(), false))
        }
        EngineEvent::Rejected { id, user, symbol, .. } => {
            Some((user.clone(), *id, symbol.clone(), false))
        }
        _ => None,
    }
}
