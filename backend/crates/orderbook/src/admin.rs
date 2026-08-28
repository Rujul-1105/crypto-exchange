//! Admin HTTP endpoints (actix-web). The orderbook server exposes:
//! - `GET /health` — liveness
//! - `GET /api/symbols` — registered symbols
//! - `GET /api/orderbook/:symbol` — depth snapshot
//! - `GET /api/trades/:symbol` — recent trades (in-memory ring; persistence
//!   via SQLite is added later)
//! - `POST /admin/snapshot` — trigger an immediate snapshot
//! - `POST /admin/place` — direct test endpoint to inject orders (Phase 4 only)

use actix_web::{web, HttpResponse, Responder};
use common::*;
use serde::Deserialize;
use std::sync::Arc;

use crate::engine::MatchingEngine;
use crate::market::SymbolRegistry;
use tokio::sync::Mutex;

pub type EngineRef = Arc<Mutex<MatchingEngine>>;

#[derive(Clone)]
pub struct AdminState {
    pub registry: SymbolRegistry,
}

pub async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({"ok": true}))
}

pub async fn list_symbols(state: web::Data<AdminState>) -> impl Responder {
    let symbols = state.registry.list().await;
    HttpResponse::Ok().json(symbols)
}

#[derive(Deserialize)]
pub struct DepthQuery {
    #[serde(default = "default_depth")]
    pub depth: usize,
}
fn default_depth() -> usize {
    20
}

pub async fn orderbook_snapshot(
    state: web::Data<AdminState>,
    path: web::Path<String>,
    query: web::Query<DepthQuery>,
) -> impl Responder {
    let symbol = path.into_inner();
    let engine = state.registry.get_or_create(symbol.clone()).await;
    let eng = engine.lock().await;
    let snap = eng.depth_snapshot(query.depth);
    HttpResponse::Ok().json(snap)
}

#[derive(Deserialize)]
pub struct TradesQuery {
    #[serde(default = "default_trades")]
    pub limit: usize,
}
fn default_trades() -> usize {
    50
}

pub async fn recent_trades(
    state: web::Data<AdminState>,
    path: web::Path<String>,
    query: web::Query<TradesQuery>,
) -> impl Responder {
    let symbol = path.into_inner();
    let engine = state.registry.get_or_create(symbol.clone()).await;
    let eng = engine.lock().await;
    let n = query.limit.min(eng.recent_trades.len());
    let start = eng.recent_trades.len().saturating_sub(n);
    let trades: Vec<Trade> = eng.recent_trades.iter().skip(start).cloned().collect();
    HttpResponse::Ok().json(trades)
}
