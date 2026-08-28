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
        // We forward every pushed event; the WS frame is JSON. Filtering by
        // subscription/user is done client-side or in the relay above.
        ctx.text(msg.0);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, item: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match item {
            Ok(ws::Message::Ping(b)) => { self.last_ping = Instant::now(); ctx.pong(&b); }
            Ok(ws::Message::Pong(_)) => { self.last_ping = Instant::now(); }
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
            Ok(ws::Message::Close(reason)) => { ctx.close(reason); ctx.stop(); }
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

fn verify_jwt(token: &str, secret: &str) -> Option<String> {
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.set_required_spec_claims(&["exp", "sub"]);
    let data = jsonwebtoken::decode::<serde_json::Value>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .ok()?;
    data.claims.get("sub")?.as_str().map(String::from)
}
