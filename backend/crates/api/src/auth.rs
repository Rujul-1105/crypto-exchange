//! Sign-In With Solana — nonce + verify → JWT.
//!
//! Flow:
//! 1. Client posts `{ pubkey }` to `/api/auth/nonce`. Server returns a random
//!    nonce bound to the pubkey (stored in Redis keyed by pubkey, TTL 10min).
//! 2. Client signs `nonce_message` (deterministic string) with their wallet.
//! 3. Client posts `{ pubkey, signature, nonce }` to `/api/auth/verify`.
//! 4. Server verifies the ed25519 signature against the nonce message; on
//!    success issues a JWT (`HS256`, 24h TTL) containing `sub = pubkey`.
//!
//! Nonce storage is Redis-backed so multiple API replicas share state —
//! before this, an in-memory `HashMap` meant the SIWS handshake broke the
//! moment the API was horizontally scaled.

use crate::redis_bus::RedisBus;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NonceRequest {
    pub pubkey: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NonceResponse {
    pub nonce: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyRequest {
    pub pubkey: String,
    pub nonce: String,
    /// base58-encoded ed25519 signature of `message` over the nonce.
    pub signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub jwt: String,
    pub pubkey: String,
    pub expires_at_unix_ms: i64,
}

/// Nonce store backed by Redis. `put` writes the full SIWS message (not just
/// the nonce) so that on `take` we can hand back the **exact bytes** the
/// wallet signed. `take` atomically reads+deletes (GETDEL), so a successful
/// verify consumes the nonce and a second attempt returns `None`.
#[derive(Clone)]
pub struct NonceStore {
    bus: RedisBus,
    ttl_secs: u64,
}

impl NonceStore {
    pub fn new(bus: RedisBus) -> Self {
        let ttl_secs: u64 = std::env::var("NONCE_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600);
        Self { bus, ttl_secs }
    }
    /// Store the full signed message so verify can recover the exact bytes.
    pub async fn put(&self, pubkey: String, message: String) {
        if let Err(e) = self.bus.put_nonce(&pubkey, &message, self.ttl_secs).await {
            tracing::warn!("put_nonce failed for {pubkey}: {e}");
        }
    }
    pub async fn take(&self, pubkey: &str) -> Option<String> {
        match self.bus.take_nonce(pubkey).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("take_nonce failed for {pubkey}: {e}");
                None
            }
        }
    }
}

/// Build the deterministic message the wallet must sign. Includes a header
/// to prevent cross-site replay and a real unix-millis timestamp so
/// `verify_fresh` can reject stale nonces.
pub fn nonce_message(pubkey: &str, nonce: &str) -> String {
    let now_ms = chrono::Utc::now().timestamp_millis();
    format!(
        "Sign in to CEX Demo\n\nPubkey: {pubkey}\nNonce: {nonce}\nIssued at: {now_ms}"
    )
}

/// Reject a nonce message whose embedded timestamp is older than the
/// configured freshness window. The window defaults to 5 minutes and is
/// overridable via `NONCE_FRESHNESS_WINDOW_MS`.
pub fn verify_fresh(message: &str) -> bool {
    let Some(line) = message.lines().find(|l| l.starts_with("Issued at: ")) else {
        return false;
    };
    let Ok(ts_ms) = line.trim_start_matches("Issued at: ").parse::<i64>() else {
        return false;
    };
    let window_ms: i64 = std::env::var("NONCE_FRESHNESS_WINDOW_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300_000);
    let now_ms = chrono::Utc::now().timestamp_millis();
    (now_ms - ts_ms).abs() <= window_ms
}

/// Verify an ed25519 signature over the nonce message.
pub fn verify_signature(pubkey_b58: &str, message: &str, signature_b58: &str) -> bool {
    let Ok(pk_bytes) = bs58::decode(pubkey_b58).into_vec() else {
        return false;
    };
    let Ok(pk_array): Result<[u8; 32], _> = pk_bytes.try_into() else {
        return false;
    };
    let Ok(sig_bytes) = bs58::decode(signature_b58).into_vec() else {
        return false;
    };
    let Ok(sig_array): Result<[u8; 64], _> = sig_bytes.try_into() else {
        return false;
    };
    let Ok(pk) = VerifyingKey::from_bytes(&pk_array) else {
        return false;
    };
    let Ok(sig) = Signature::from_slice(&sig_array) else {
        return false;
    };
    pk.verify(message.as_bytes(), &sig).is_ok()
}

pub fn new_nonce() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    bs58::encode(bytes).into_string()
}

/// Decode an HS256 JWT and return the `sub` claim (the user's base58
/// pubkey). Returns `None` if the token is invalid or the secret doesn't
/// match. Used by both the REST routes (via `require_user`) and the WS
/// handler so the verify logic lives in exactly one place.
pub fn verify_jwt(token: &str, secret: &str) -> Option<String> {
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
