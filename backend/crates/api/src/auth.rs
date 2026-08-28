//! Sign-In With Solana — nonce + verify → JWT.
//!
//! Flow:
//! 1. Client posts `{ pubkey }` to `/api/auth/nonce`. Server returns a random
//!    nonce bound to the pubkey (stored in-memory keyed by pubkey).
//! 2. Client signs `nonce_message` (deterministic string) with their wallet.
//! 3. Client posts `{ pubkey, signature, nonce }` to `/api/auth/verify`.
//! 4. Server verifies the ed25519 signature against the nonce message; on
//!    success issues a JWT (`HS256`, 24h TTL) containing `sub = pubkey`.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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

#[derive(Default, Clone)]
pub struct NonceStore {
    inner: Arc<RwLock<HashMap<String, String>>>, // pubkey -> nonce
}

impl NonceStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn put(&self, pubkey: String, nonce: String) {
        self.inner.write().await.insert(pubkey, nonce);
    }
    pub async fn take(&self, pubkey: &str) -> Option<String> {
        self.inner.write().await.remove(pubkey)
    }
}

/// Build the deterministic message the wallet must sign. Includes a header
/// to prevent cross-site replay.
pub fn nonce_message(pubkey: &str, nonce: &str) -> String {
    format!("Sign in to CEX Demo\n\nPubkey: {pubkey}\nNonce: {nonce}\nIssued at: now")
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
