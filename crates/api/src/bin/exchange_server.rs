//! `exchange-server` — binary entry point for the Phase 5 HTTP server.
//!
//! On startup:
//! 1. Create or open the WAL at the configured path.
//! 2. Bootstrap the OrderService by replaying WAL events.
//! 3. Bind an axum router on `0.0.0.0:8080` (configurable via env var).
//!
//! Run: `cargo run --bin exchange-server`

use std::path::PathBuf;
use std::sync::Arc;

use api::http::{self, AppState};
use api::OrderService;
use ledger::InMemoryLedger;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wal_path: PathBuf = std::env::var("WAL_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./exchange.wal"));
    let bind: std::net::SocketAddr = std::env::var("BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse()?;

    let service = Arc::new(OrderService::new(&wal_path).map_err(|e| e.to_string())?);
    // Replay WAL into the in-memory state.
    if wal_path.exists() {
        match service.bootstrap_from_wal(&wal_path) {
            Ok(n) if n > 0 => println!("[startup] replayed {} WAL events", n),
            Ok(_) => {}
            Err(e) => eprintln!("[startup] WAL replay error: {}", e),
        }
    }

    let state = AppState {
        users: service.users.clone(),
        ledger: service.ledger.clone(),
        service: service.clone(),
        actors: service.actors.clone(),
        wal: service.wal.clone(),
        idempotency: service.idempotency.clone(),
        rate_limit: service.rate_limit.clone(),
        jwt_secret: std::env::var("JWT_SECRET")
            .map(|s| s.into_bytes())
            .unwrap_or_else(|_| b"dev-secret-change-me".to_vec()),
    };

    println!("[startup] listening on http://{}", bind);
    http::run_server(bind, state).await?;
    Ok(())
}