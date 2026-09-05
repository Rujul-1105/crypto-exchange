//! WebSocket endpoint — clients subscribe to channels
//! (`book:<sym>`, `trades:<sym>`, `candles:<sym>:<interval>`, `orders:<sym>`),
//! server relays matching events from `events:outgoing`.
//!
//! Uses `actix-web-actors::ws` so we get a proper Actor lifecycle. Each
//! connection spawns:
//!   - a "feed" task that XREADs `events:outgoing` and pushes to the session's
//!     outbound channel
//!   - the session actor that handles inbound client messages (subscribe/unsubscribe/ping)

use actix::{Actor, ActorContext, AsyncContext, Handler, Message, StreamHandler};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use common::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::auth::verify_jwt;
use crate::redis_bus::RedisBus;
use crate::routes::ApiState;

#[derive(Deserialize)]
pub struct WsQuery {
    pub token: Option<String>,
}

/// Messages from the client.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientMessage {
    Subscribe { channel: String },
    Unsubscribe { channel: String },
    Ping { ts: i64 },
}

#[derive(Message)]
#[rtype(result = "()")]
struct PushText(String);

pub struct WsSession {
    pub bus: RedisBus,
    pub user_pubkey: Option<String>,
    pub subscriptions: HashSet<String>,
    pub last_ping: Instant,
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Heartbeat every 15s — if no pong in 30s, close.
        ctx.run_interval(Duration::from_secs(15), |act, ctx| {
            if Instant::now().duration_since(act.last_ping) > Duration::from_secs(30) {
                ctx.stop();
                return;
            }
            ctx.ping(b"");
        });

        // Spawn the feed task — XREADs events:outgoing forever.
        let addr = ctx.address();
        let bus = self.bus.clone();
        actix::spawn(async move {
            let mut last_id = "0".to_string();
            loop {
                match bus.read_events(&last_id, 5000).await {
                    Ok(events) => {
                        for (stream_id, env) in events {
                            last_id = stream_id;
                            if let Ok(s) = serde_json::to_string(&env) {
                                let _ = addr.do_send(PushText(s));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("ws read_events: {e}");
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        });
    }
}

impl Handler<PushText> for WsSession {
    type Result = ();
    fn handle(&mut self, msg: PushText, ctx: &mut Self::Context) {
        // Filter the pushed event against this session's subscriptions before
        // forwarding. Parse the envelope first so we know the symbol and
        // event type.
        let env: EventEnvelope = match serde_json::from_str(&msg.0) {
            Ok(e) => e,
            Err(_) => {
                // Unparseable — forward as-is so the client sees the error.
                ctx.text(msg.0);
                return;
            }
        };
        if !should_forward(&env, &self.subscriptions, self.user_pubkey.as_deref()) {
            return;
        }
        ctx.text(msg.0);
    }
}

/// Map an `EventEnvelope` to the channels it would belong to (`book:<sym>`,
/// `trades:<sym>`, `candles:<sym>:<interval>`, `orders:<sym>`). An envelope
/// may map to multiple channels (e.g. `Fill` lives on both `orders:*` and
/// `trades:*`); we forward if **any** matches.
fn envelope_channels(env: &EventEnvelope) -> Vec<String> {
    match &env.event {
        EngineEvent::BookDelta { symbol, .. } => vec![format!("book:{symbol}")],
        EngineEvent::Trade { trade } => vec![format!("trades:{}", trade.symbol)],
        EngineEvent::CandleUpdate { symbol, candle, .. } => {
            vec![format!("candles:{}:{}", symbol, candle.interval)]
        }
        EngineEvent::Fill { trade, .. } => vec![
            format!("orders:{}", trade.symbol),
            format!("trades:{}", trade.symbol),
        ],
        EngineEvent::Accepted { order } => vec![format!("orders:{}", order.symbol)],
        EngineEvent::Amended { order, .. } => vec![format!("orders:{}", order.symbol)],
        EngineEvent::Cancelled { symbol, .. } => vec![format!("orders:{symbol}")],
        EngineEvent::Rejected { symbol, .. } => vec![format!("orders:{symbol}")],
        EngineEvent::SettleUpdate { symbol, .. } => vec![format!("orders:{symbol}")],
    }
}

/// For order-scoped events, return the pubkey whose orders they describe.
/// Used to enforce the per-user filter on `orders:*` channels.
fn envelope_owner(env: &EventEnvelope) -> Option<&str> {
    match &env.event {
        EngineEvent::Accepted { order } => Some(&order.user),
        EngineEvent::Amended { order, .. } => Some(&order.user),
        EngineEvent::Cancelled { user, .. } => Some(user),
        EngineEvent::Rejected { user, .. } => Some(user),
        EngineEvent::Fill { trade, .. } => Some(&trade.buyer),
        _ => None,
    }
}

/// Decide whether the session should receive this envelope based on its
/// subscriptions and (for `orders:*`) its user pubkey.
fn should_forward(
    env: &EventEnvelope,
    subs: &HashSet<String>,
    user_pubkey: Option<&str>,
) -> bool {
    let channels = envelope_channels(env);
    let mut any_match = false;
    for ch in &channels {
        if !subs.contains(ch) {
            continue;
        }
        // For order-scoped channels, enforce the per-user filter.
        if ch.starts_with("orders:") {
            if let Some(me) = user_pubkey {
                match envelope_owner(env) {
                    Some(owner) if owner == me => return true,
                    _ => continue,
                }
            }
            // No user_pubkey means the session didn't authenticate; still
            // forward `orders:*` events (anonymous watch mode). Production
            // should require auth, but the demo allows unauthenticated WS.
            return true;
        }
        any_match = true;
    }
    any_match
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, item: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match item {
            Ok(ws::Message::Ping(b)) => {
                self.last_ping = Instant::now();
                ctx.pong(&b);
            }
            Ok(ws::Message::Pong(_)) => {
                self.last_ping = Instant::now();
            }
            Ok(ws::Message::Text(t)) => {
                self.last_ping = Instant::now();
                match serde_json::from_str::<ClientMessage>(&t) {
                    Ok(ClientMessage::Subscribe { channel }) => {
                        self.subscriptions.insert(channel);
                    }
                    Ok(ClientMessage::Unsubscribe { channel }) => {
                        self.subscriptions.remove(&channel);
                    }
                    Ok(ClientMessage::Ping { ts }) => {
                        ctx.text(format!(r#"{{"op":"pong","ts":{ts}}}"#));
                    }
                    Err(_) => {
                        ctx.text(r#"{"type":"error","code":"bad_json"}"#);
                    }
                }
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => {}
        }
    }
}

pub async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<ApiState>,
    query: web::Query<WsQuery>,
) -> HttpResponse {
    let user_pubkey = query
        .token
        .as_deref()
        .and_then(|t| verify_jwt(t, &state.jwt_secret));

    let session = WsSession {
        bus: state.bus.clone(),
        user_pubkey,
        subscriptions: HashSet::new(),
        last_ping: Instant::now(),
    };
    ws::start(session, &req, stream).unwrap_or_else(|e| {
        tracing::error!("ws start: {e}");
        HttpResponse::InternalServerError().body(format!("ws: {e}"))
    })
}
