//! Per-user idempotency cache.
//!
//! When a client submits an order with an `Idempotency-Key` header, we
//! remember the response keyed by `(user_id, key)` and replay it on
//! subsequent requests that carry the same key. This protects against
//! network retries causing duplicate orders.
//!
//! Phase 5 keeps this in-memory. Eviction policy: drop entries older
//! than `ttl_secs` on lookup. No explicit GC loop.
//!
//! This is intentionally simple — the deliverable is "dedupe retried
//! requests". A production system would use Redis or a DB-backed
//! store; Phase 6+ can revisit.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Stored response, returned to clients on duplicate requests.
#[derive(Debug, Clone)]
pub struct StoredResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

struct Entry {
    body: serde_json::Value,
    expires_at_unix: u64,
}

pub struct IdempotencyCache {
    ttl: Duration,
    inner: Mutex<HashMap<(u64, String), Entry>>,
}

impl IdempotencyCache {
    pub fn new(ttl_secs: u64) -> Self {
        IdempotencyCache {
            ttl: Duration::from_secs(ttl_secs),
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Look up a cached response. Evicts expired entries lazily.
    pub fn lookup(&self, user_id: u64, key: &str) -> Option<StoredResponse> {
        let mut guard = self.inner.lock().ok()?;
        let now = Self::now_unix();
        // Evict this key if expired.
        if let Some(entry) = guard.get(&(user_id, key.to_owned())) {
            if entry.expires_at_unix < now {
                guard.remove(&(user_id, key.to_owned()));
                return None;
            }
            return Some(StoredResponse {
                status: 200,
                body: entry.body.clone(),
            });
        }
        None
    }

    /// Store a response for replay.
    pub fn store(&self, user_id: u64, key: &str, body: serde_json::Value) {
        let Ok(mut guard) = self.inner.lock() else { return };
        guard.insert(
            (user_id, key.to_owned()),
            Entry {
                body,
                expires_at_unix: Self::now_unix() + self.ttl.as_secs(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn first_lookup_is_empty_then_subsequent_returns_cached() {
        let cache = IdempotencyCache::new(60);
        assert!(cache.lookup(1, "key-1").is_none());
        cache.store(1, "key-1", json!({"order_id": 42}));
        let cached = cache.lookup(1, "key-1").expect("cached");
        assert_eq!(cached.body, json!({"order_id": 42}));
    }

    #[test]
    fn keys_are_per_user() {
        let cache = IdempotencyCache::new(60);
        cache.store(1, "key", json!("from-1"));
        cache.store(2, "key", json!("from-2"));
        assert_eq!(cache.lookup(1, "key").unwrap().body, json!("from-1"));
        assert_eq!(cache.lookup(2, "key").unwrap().body, json!("from-2"));
    }

    #[test]
    fn expired_entries_evict_on_lookup() {
        let cache = IdempotencyCache::new(0); // ttl 0 → expired immediately
        cache.store(1, "k", json!("v"));
        // Force clock past expiry
        std::thread::sleep(Duration::from_millis(1100));
        assert!(cache.lookup(1, "k").is_none());
    }
}