//! REST endpoints — read endpoints proxy to the orderbook admin server,
//! mutating endpoints publish to `orders:incoming`.

use actix_web::{web, HttpResponse, Responder};
use common::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::auth::{self, NonceRequest, NonceResponse, NonceStore, VerifyRequest, VerifyResponse};
use crate::redis_bus::RedisBus;

/// `user → order_id → symbol`. Populated by the events:outgoing tracker
/// spawned in `main.rs`; consulted by `cancel_order` / `amend_order` to
/// resolve the symbol for an order id without a local user-orders index.
pub type UserOrdersIndex = Arc<RwLock<HashMap<String, HashMap<u64, String>>>>;

#[derive(Clone)]
pub struct ApiState {
    pub bus: RedisBus,
    pub orderbook_admin: String,
    pub allowed_origins: Vec<String>,
    pub jwt_secret: String,
    pub nonce_store: NonceStore,
    pub user_orders: UserOrdersIndex,
}

pub async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({"ok": true}))
}

pub async fn symbols(state: web::Data<ApiState>) -> impl Responder {
    let url = format!("{}/api/symbols", state.orderbook_admin);
    proxy_get(&url).await
}

#[derive(Deserialize)]
pub struct DepthQuery {
    #[serde(default = "default_depth")]
    pub depth: usize,
}
fn default_depth() -> usize {
    20
}

pub async fn orderbook(
    state: web::Data<ApiState>,
    path: web::Path<String>,
    query: web::Query<DepthQuery>,
) -> impl Responder {
    let symbol = path.into_inner();
    let url = format!(
        "{}/api/orderbook/{}?depth={}",
        state.orderbook_admin, symbol, query.depth
    );
    proxy_get(&url).await
}

#[derive(Deserialize)]
pub struct TradesQuery {
    #[serde(default = "default_trades")]
    pub limit: usize,
}
fn default_trades() -> usize {
    50
}

pub async fn trades(
    state: web::Data<ApiState>,
    path: web::Path<String>,
    query: web::Query<TradesQuery>,
) -> impl Responder {
    let symbol = path.into_inner();
    let url = format!(
        "{}/api/trades/{}?limit={}",
        state.orderbook_admin, symbol, query.limit
    );
    proxy_get(&url).await
}

#[derive(Deserialize)]
pub struct CandlesQuery {
    #[serde(default = "default_candles_limit")]
    pub limit: usize,
}
fn default_candles_limit() -> usize {
    500
}

pub async fn candles(
    state: web::Data<ApiState>,
    path: web::Path<(String, String)>,
    query: web::Query<CandlesQuery>,
) -> impl Responder {
    let (symbol, interval) = path.into_inner();
    let url = format!(
        "{}/api/candles/{}/{}?limit={}",
        state.orderbook_admin, symbol, interval, query.limit
    );
    proxy_get(&url).await
}

// ── Auth ─────────────────────────────────────────────────────

pub async fn auth_nonce(
    state: web::Data<ApiState>,
    body: web::Json<NonceRequest>,
) -> impl Responder {
    let nonce = auth::new_nonce();
    let pubkey = body.pubkey.clone();
    let message = auth::nonce_message(&pubkey, &nonce);
    state.nonce_store.put(pubkey.clone(), nonce.clone()).await;
    HttpResponse::Ok().json(NonceResponse { nonce, message })
}

pub async fn auth_verify(
    state: web::Data<ApiState>,
    body: web::Json<VerifyRequest>,
) -> impl Responder {
    let stored = match state.nonce_store.take(&body.pubkey).await {
        Some(n) if n == body.nonce => n,
        _ => {
            return HttpResponse::Unauthorized()
                .json(serde_json::json!({"error": "invalid_or_expired_nonce"}))
        }
    };
    let message = auth::nonce_message(&body.pubkey, &stored);
    // Reject stale signed payloads so a captured nonce can't be replayed
    // beyond the freshness window.
    if !auth::verify_fresh(&message) {
        return HttpResponse::Unauthorized()
            .json(serde_json::json!({"error": "stale_nonce"}));
    }
    if !auth::verify_signature(&body.pubkey, &message, &body.signature) {
        return HttpResponse::Unauthorized().json(serde_json::json!({"error": "bad_signature"}));
    }
    let ttl_hours: i64 = std::env::var("JWT_TTL_HOURS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);
    let now = chrono::Utc::now().timestamp();
    let exp = now + ttl_hours * 3600;
    let claims = serde_json::json!({
        "sub": body.pubkey,
        "iat": now,
        "exp": exp,
    });
    let token = match jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    ) {
        Ok(t) => t,
        Err(e) => return HttpResponse::InternalServerError().body(format!("jwt: {e}")),
    };
    HttpResponse::Ok().json(VerifyResponse {
        jwt: token,
        pubkey: body.pubkey.clone(),
        expires_at_unix_ms: exp * 1000,
    })
}

// ── Orders ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PlaceOrderBody {
    pub symbol: String,
    pub side: Side,
    #[serde(flatten)]
    pub order_type: OrderType,
    #[serde(default = "default_tif")]
    pub tif: TimeInForce,
    pub price: Option<Price>,
    pub quantity: Quantity,
}

fn default_tif() -> TimeInForce {
    TimeInForce::Gtc
}

pub async fn place_order(
    state: web::Data<ApiState>,
    body: web::Json<PlaceOrderBody>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    let user = match require_user(&req, &state.jwt_secret) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let cmd = OrderCommand::Place(PlaceOrder {
        user,
        symbol: body.symbol.clone(),
        side: body.side,
        order_type: body.order_type.clone(),
        tif: body.tif,
        price: body.price,
        quantity: body.quantity,
        client_order_id: None,
    });
    match state.bus.push_order(&cmd).await {
        Ok(stream_id) => HttpResponse::Accepted()
            .json(serde_json::json!({"stream_id": stream_id, "status": "queued"})),
        Err(e) => HttpResponse::InternalServerError().body(format!("xadd: {e}")),
    }
}

pub async fn cancel_order(
    state: web::Data<ApiState>,
    path: web::Path<u64>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    let user = match require_user(&req, &state.jwt_secret) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let order_id = path.into_inner();
    // Look up the order's symbol from the local user-orders index (kept
    // in sync with `events:outgoing`). If we don't have it, the order is
    // either unknown to us or already terminal — return 404.
    let symbol = {
        let map = state.user_orders.read().await;
        map.get(&user).and_then(|m| m.get(&order_id)).cloned()
    };
    let symbol = match symbol {
        Some(s) => s,
        None => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({"error": "order_not_found"}))
        }
    };
    let cmd = OrderCommand::Cancel(CancelOrder {
        user,
        symbol,
        order_id,
    });
    match state.bus.push_order(&cmd).await {
        Ok(id) => HttpResponse::Accepted().json(serde_json::json!({"stream_id": id})),
        Err(e) => HttpResponse::InternalServerError().body(format!("xadd: {e}")),
    }
}

pub async fn amend_order(
    state: web::Data<ApiState>,
    path: web::Path<u64>,
    body: web::Json<serde_json::Value>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    let user = match require_user(&req, &state.jwt_secret) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let order_id = path.into_inner();
    let new_price: Price = match body
        .get("price")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(p) => p,
        None => return HttpResponse::BadRequest().body("missing price"),
    };
    let new_qty: Quantity = match body
        .get("quantity")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
    {
        Some(q) => q,
        None => return HttpResponse::BadRequest().body("missing quantity"),
    };
    let symbol = {
        let map = state.user_orders.read().await;
        map.get(&user).and_then(|m| m.get(&order_id)).cloned()
    };
    let symbol = match symbol {
        Some(s) => s,
        None => {
            return HttpResponse::NotFound()
                .json(serde_json::json!({"error": "order_not_found"}))
        }
    };
    let cmd = OrderCommand::Amend(AmendOrder {
        user,
        symbol,
        order_id,
        new_price,
        new_quantity: new_qty,
    });
    match state.bus.push_order(&cmd).await {
        Ok(id) => HttpResponse::Accepted().json(serde_json::json!({"stream_id": id})),
        Err(e) => HttpResponse::InternalServerError().body(format!("xadd: {e}")),
    }
}

pub async fn get_order(state: web::Data<ApiState>, path: web::Path<u64>) -> impl Responder {
    let id = path.into_inner();
    let url = format!("{}/api/orders/{}", state.orderbook_admin, id);
    proxy_get(&url).await
}

pub async fn list_orders(
    state: web::Data<ApiState>,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    // Whitelist the query keys we forward to the orderbook admin. Anything
    // else gets a 400 so clients can't smuggle extra parameters through us.
    const ALLOWED: &[&str] = &["user", "symbol", "status"];
    let mut qs_parts: Vec<String> = Vec::new();
    for key in ALLOWED {
        if let Some(v) = query.get(*key) {
            qs_parts.push(format!("{key}={v}"));
        }
    }
    for k in query.keys() {
        if !ALLOWED.contains(&k.as_str()) {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "unknown_query_key",
                "key": k,
            }));
        }
    }
    let url = format!("{}/api/orders?{}", state.orderbook_admin, qs_parts.join("&"));
    proxy_get(&url).await
}

pub async fn balances(state: web::Data<ApiState>, req: actix_web::HttpRequest) -> impl Responder {
    let user = match require_user(&req, &state.jwt_secret) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    // Proxy to the orderbook admin balances endpoint (placeholder).
    let url = format!("{}/api/balances?user={}", state.orderbook_admin, user);
    proxy_get(&url).await
}

// ── Helpers ─────────────────────────────────────────────────

fn require_user(req: &actix_web::HttpRequest, secret: &str) -> Result<String, HttpResponse> {
    let auth_header = req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
    if token.is_empty() {
        return Err(HttpResponse::Unauthorized().json(serde_json::json!({"error": "missing_jwt"})));
    }
    auth::verify_jwt(token, secret)
        .ok_or_else(|| HttpResponse::Unauthorized().json(serde_json::json!({"error": "bad_jwt"})))
}

async fn proxy_get(url: &str) -> HttpResponse {
    match reqwest_get(url).await {
        Ok((status, body)) => HttpResponse::build(
            actix_web::http::StatusCode::from_u16(status)
                .unwrap_or(actix_web::http::StatusCode::BAD_GATEWAY),
        )
        .content_type("application/json")
        .body(body),
        Err(e) => HttpResponse::BadGateway().body(format!("upstream: {e}")),
    }
}

async fn reqwest_get(url: &str) -> anyhow::Result<(u16, String)> {
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?
        .get(url)
        .send()
        .await?;
    let status = resp.status().as_u16();
    let body = resp.text().await?;
    Ok((status, body))
}

// Extension trait removed — NonceStore is now a field on ApiState.
