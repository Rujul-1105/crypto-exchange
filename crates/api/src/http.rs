//! Axum HTTP layer: routes, extractors, JSON responses.
//!
//! Phase 5 wires up the in-process pieces:
//!
//! - `AppState`: shared state (users, ledger, service, actors, WAL, idempotency,
//!   rate limiter, JWT secret).
//! - `AuthContext` extractor: parses `Authorization: Bearer <jwt>` and
//!   verifies via `AuthContext::from_token`.
//! - Handlers: thin JSON wrappers around the auth-aware functions in
//!   `crate::handlers` and around `OrderService`.
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{FromRequestParts, Path, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use common::{OrderId, OrderKind, Price, Qty, Side, Symbol, Timestamp};
use ledger::{Asset, Ledger, UserId};

use crate::auth::{AuthContext, AuthError, InMemoryUserStore, Role, User};
use crate::error::ApiError;
use crate::idempotency::IdempotencyCache;
use crate::ratelimit::RateLimiter;
use crate::service::OrderService;
use crate::wal::Wal;

// ============== AppState ==============

#[derive(Clone)]
pub struct AppState {
    pub users: Arc<std::sync::Mutex<InMemoryUserStore>>,
    pub ledger: Arc<std::sync::Mutex<ledger::InMemoryLedger>>,
    pub service: Arc<OrderService>,
    pub actors: crate::service::ActorRegistry,
    pub wal: Arc<std::sync::Mutex<Wal>>,
    pub idempotency: Arc<IdempotencyCache>,
    pub rate_limit: Arc<RateLimiter>,
    pub jwt_secret: Vec<u8>,
}

// ============== AuthContext extractor ==============

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header_val = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::Auth(AuthError::InvalidToken("missing header".into())))?;

        let token = header_val
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Auth(AuthError::InvalidToken("malformed".into())))?;

        let ctx = AuthContext::from_token(token, &state.jwt_secret).map_err(ApiError::Auth)?;
        Ok(ctx)
    }
}

// ============== JSON request / response types ==============

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub role: Option<Role>,
}

#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub user_id: u64,
    pub username: String,
    pub role: Role,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user_id: u64,
    pub username: String,
    pub role: Role,
    pub token: String,
    pub expires_at: u64,
}

#[derive(Debug, Deserialize)]
pub struct SubmitOrderRequest {
    pub symbol: String,
    /// "buy" or "sell". String to keep common's dep tree clean.
    pub side: String,
    /// "limit" or "market".
    pub kind: String,
    pub price: Option<u64>,
    pub qty: u64,
    /// Optional client-supplied idempotency key. Repeated requests with
    /// the same key (per user) within the TTL replay the first response.
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

impl SubmitOrderRequest {
    fn side_parsed(&self) -> Result<Side, ApiError> {
        match self.side.as_str() {
            "buy" => Ok(Side::Buy),
            "sell" => Ok(Side::Sell),
            _ => Err(ApiError::Auth(AuthError::Internal(format!(
                "invalid side {:?}",
                self.side
            )))),
        }
    }
    fn kind_parsed(&self) -> Result<OrderKind, ApiError> {
        match self.kind.as_str() {
            "limit" => Ok(OrderKind::Limit),
            "market" => Ok(OrderKind::Market),
            _ => Err(ApiError::Auth(AuthError::Internal(format!(
                "invalid kind {:?}",
                self.kind
            )))),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SubmitOrderResponse {
    pub order_id: u64,
    pub fills: Vec<FillJson>,
    pub resting: bool,
    pub cancelled_remainder: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct FillJson {
    pub maker_order_id: u64,
    pub taker_order_id: u64,
    pub price: u64,
    pub qty: u64,
}

#[derive(Debug, Serialize)]
pub struct BalanceJson {
    pub user_id: u64,
    pub asset: String,
    pub available: u64,
    pub locked: u64,
}

#[derive(Debug, Serialize)]
pub struct BookLevel {
    pub price: u64,
    pub qty: u64,
}

#[derive(Debug, Serialize)]
pub struct BookSnapshot {
    pub symbol: String,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
}

#[derive(Debug, Deserialize)]
pub struct AdminAdjustBalanceRequest {
    pub user_id: u64,
    pub asset: String,
    pub amount: u64,
    pub is_deposit: bool,
}

#[derive(Debug, Deserialize)]
pub struct AdminCreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: Role,
}

/// Response for user creation. Strips the password hash so we never
/// serialize it (the hash is sensitive even though `Debug` redacts it).
#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub user_id: u64,
    pub username: String,
    pub role: Role,
}

// ============== Handlers ==============

async fn health() -> &'static str {
    "ok"
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, ApiError> {
    let role = req.role.unwrap_or(Role::User);
    let mut users = state.users.lock().unwrap();
    let user = crate::handlers::register(&mut *users, &req.username, &req.password, role)
        .map_err(ApiError::Auth)?;
    Ok(Json(RegisterResponse {
        user_id: user.id.0,
        username: user.username.clone(),
        role: user.role,
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let users = state.users.lock().unwrap();
    let (user, token) =
        crate::handlers::login(&*users, &req.username, &req.password, &state.jwt_secret, 3600)
            .map_err(ApiError::Auth)?;
    let (_tok, exp) = crate::issue_token(user.id, user.role, &state.jwt_secret, 3600)
        .map_err(ApiError::Auth)?;
    Ok(Json(LoginResponse {
        user_id: user.id.0,
        username: user.username.clone(),
        role: user.role,
        token,
        expires_at: exp,
    }))
}

async fn submit_order(
    State(state): State<AppState>,
    ctx: AuthContext,
    // Re-typed: the (State, AuthContext) extractors must run before the
    // Json body extractor so missing/invalid Authorization headers
    // surface as 401 (auth failure) rather than 500 (deserialization
    // error). The body parser still validates, just after auth.
    body: axum::extract::Json<SubmitOrderRequest>,
) -> Result<Response, ApiError> {
    // Idempotency: check before doing any work.
    if let Some(key) = &body.idempotency_key {
        if let Some(cached) = state.idempotency.lookup(ctx.user_id.0, key) {
            return Ok((StatusCode::OK, Json(cached.body)).into_response());
        }
    }

    let symbol = Symbol::from(body.symbol.as_str());
    let side = body.side_parsed()?;
    let kind = body.kind_parsed()?;
    let qty = Qty(body.qty);
    let price = body.price.map(Price);
    let ts = Timestamp(0);

    let result = state
        .service
        .submit_order(ctx.user_id, symbol, side, kind, price, qty, ts)
        .await
        .map_err(|e| ApiError::Auth(AuthError::Internal(format!("order: {}", e))))?;

    let resp = SubmitOrderResponse {
        order_id: result.order_id.0,
        fills: result
            .fills
            .iter()
            .map(|f| FillJson {
                maker_order_id: f.maker_order_id.0,
                taker_order_id: f.taker_order_id.0,
                price: f.price,
                qty: f.qty,
            })
            .collect(),
        resting: result.resting,
        cancelled_remainder: result.cancelled_remainder,
    };
    let body_json = serde_json::to_value(&resp).map_err(|e| {
        ApiError::Auth(AuthError::Internal(format!("json: {}", e)))
    })?;
    if let Some(key) = &body.idempotency_key {
        state
            .idempotency
            .store(ctx.user_id.0, key, body_json.clone());
    }
    Ok((StatusCode::CREATED, Json(resp)).into_response())
}

async fn cancel_order(
    State(state): State<AppState>,
    ctx: AuthContext,
    Path(order_id): Path<u64>,
) -> Result<Json<()>, ApiError> {
    let actor_symbols = state.actors.symbols();
    let id = OrderId(order_id);
    for sym in actor_symbols {
        let actor = state.actors.get_or_create(sym);
        match actor.cancel(id).await {
            Ok(()) => {
                let _ = (&mut *state.ledger.lock().unwrap()).cancel(id);
                return Ok(Json(()));
            }
            Err(_) => continue,
        }
    }
    Err(ApiError::Auth(AuthError::UnknownOrder(id)))
}

async fn get_balances(
    State(state): State<AppState>,
    ctx: AuthContext,
) -> Result<Json<Vec<BalanceJson>>, ApiError> {
    let ledger = state.ledger.lock().unwrap();
    let mut out = Vec::new();
    for asset in [Asset::from("USDC"), Asset::from("BTC")] {
        let acct = (&*ledger).account(ctx.user_id, asset.clone());
        out.push(BalanceJson {
            user_id: ctx.user_id.0,
            asset: acct.asset.to_string(),
            available: acct.available,
            locked: acct.locked,
        });
    }
    Ok(Json(out))
}

async fn get_book(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> Result<Json<BookSnapshot>, ApiError> {
    // Public endpoint — anyone can view the book.
    let sym = Symbol::from(symbol.as_str());
    let actor = state.actors.get_or_create(sym.clone());
    let snapshot = actor.snapshot().await;
    Ok(Json(BookSnapshot {
        symbol: sym.as_str().to_owned(),
        bids: snapshot
            .bids
            .iter()
            .map(|(p, q)| BookLevel { price: p.0, qty: q.0 })
            .collect(),
        asks: snapshot
            .asks
            .iter()
            .map(|(p, q)| BookLevel { price: p.0, qty: q.0 })
            .collect(),
    }))
}

async fn admin_adjust_balance(
    State(state): State<AppState>,
    ctx: AuthContext,
    Json(req): Json<AdminAdjustBalanceRequest>,
) -> Result<Json<()>, ApiError> {
    let users = state.users.lock().unwrap();
    let mut ledger = state.ledger.lock().unwrap();
    let amount = common::Qty(req.amount);
    crate::handlers::admin_adjust_balance(
        &ctx,
        &*users,
        &mut *ledger,
        UserId(req.user_id),
        Asset::from(req.asset.as_str()),
        amount,
        req.is_deposit,
    )
    .map_err(ApiError::Auth)?;
    Ok(Json(()))
}

async fn admin_create_user(
    State(state): State<AppState>,
    ctx: AuthContext,
    Json(req): Json<AdminCreateUserRequest>,
) -> Result<Json<UserResponse>, ApiError> {
    let mut users = state.users.lock().unwrap();
    let user = crate::handlers::admin_create_user(
        &ctx,
        &mut *users,
        &req.username,
        &req.password,
        req.role,
    )
    .map_err(ApiError::Auth)?;
    Ok(Json(UserResponse {
        user_id: user.id.0,
        username: user.username.clone(),
        role: user.role,
    }))
}

#[derive(Debug, Deserialize)]
pub struct AdminRegisterSymbolRequest {
    pub symbol: String,
}

async fn admin_register_symbol(
    State(state): State<AppState>,
    ctx: AuthContext,
    Json(req): Json<AdminRegisterSymbolRequest>,
) -> Result<Json<SymbolResponse>, ApiError> {
    let sym = crate::handlers::admin_register_symbol(&ctx, &req.symbol).map_err(ApiError::Auth)?;
    state.actors.get_or_create(sym.clone());
    Ok(Json(SymbolResponse {
        symbol: sym.as_str().to_owned(),
    }))
}

#[derive(Debug, Serialize)]
pub struct SymbolResponse {
    pub symbol: String,
}

async fn admin_list_symbols(State(state): State<AppState>) -> Json<Vec<String>> {
    Json(state.actors.symbols().iter().map(|s| s.as_str().to_owned()).collect())
}

// ============== Router ==============

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/orders", post(submit_order))
        .route("/orders/:order_id", delete(cancel_order))
        .route("/balances", get(get_balances))
        .route("/book/:symbol", get(get_book))
        .route("/admin/balances", post(admin_adjust_balance))
        .route("/admin/users", post(admin_create_user))
        .route(
            "/admin/symbols",
            post(admin_register_symbol).get(admin_list_symbols),
        )
        .with_state(state)
}

pub async fn run_server(addr: SocketAddr, state: AppState) -> std::io::Result<()> {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await
}

// ============== Convenience newtype for Path(usize) into OrderId ==============
// (Replaced with direct OrderId use in the cancel_order handler.)