//! API server (actix-web) — stateless REST + WebSocket. Bridges clients to
//! the orderbook server via Redis Streams and an internal HTTP proxy to the
//! orderbook's admin endpoints for reads.

use actix_cors::Cors;
use actix_web::{middleware, web, App, HttpServer};
use tracing_subscriber::EnvFilter;

mod auth;
mod redis_bus;
mod routes;
mod ws;

use redis_bus::RedisBus;

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
    let state = routes::ApiState {
        bus,
        orderbook_admin,
        allowed_origins: allowed_origins.clone(),
        jwt_secret: jwt_secret.clone(),
        nonce_store: crate::auth::NonceStore::new(),
    };
    let data = web::Data::new(state);

    tracing::info!("api server listening on {bind}");
    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);
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
