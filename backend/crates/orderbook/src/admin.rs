//! Admin HTTP endpoints (actix-web). The orderbook server exposes:
//! - `GET /health` — liveness
//! - `GET /api/symbols` — registered symbols
//! - `GET /api/orderbook/:symbol` — depth snapshot
//! - `GET /api/trades/:symbol` — recent trades (in-memory ring; persistence
//!   via SQLite is added later)
//! - `GET /api/candles/:symbol/:interval` — historical candles (closed
//!   buckets + current open bucket)

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
    let Some(engine) = state.registry.get(&symbol).await else {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": "unknown_symbol",
            "symbol": symbol,
        }));
    };
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
    let Some(engine) = state.registry.get(&symbol).await else {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": "unknown_symbol",
            "symbol": symbol,
        }));
    };
    let eng = engine.lock().await;
    let n = query.limit.min(eng.recent_trades.len());
    let start = eng.recent_trades.len().saturating_sub(n);
    let trades: Vec<Trade> = eng.recent_trades.iter().skip(start).cloned().collect();
    HttpResponse::Ok().json(trades)
}

#[derive(Deserialize)]
pub struct CandlesQuery {
    #[serde(default = "default_candle_limit")]
    pub limit: usize,
}
fn default_candle_limit() -> usize {
    500
}

/// Historical candles for `(symbol, interval)` — the closed-bucket ring
/// followed by the in-progress open bucket. Used by the chart's REST
/// history hydration.
pub async fn candles(
    state: web::Data<AdminState>,
    path: web::Path<(String, String)>,
    query: web::Query<CandlesQuery>,
) -> impl Responder {
    let (symbol, interval_str) = path.into_inner();
    let Some(interval) = CandleInterval::from_str(&interval_str) else {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "bad_interval",
            "interval": interval_str,
        }));
    };
    let Some(engine) = state.registry.get(&symbol).await else {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": "unknown_symbol",
            "symbol": symbol,
        }));
    };
    let eng = engine.lock().await;
    let all = eng.candle_aggregator.history_with_open(interval);
    let n = query.limit.min(all.len());
    let start = all.len().saturating_sub(n);
    HttpResponse::Ok().json(&all[start..])
}
