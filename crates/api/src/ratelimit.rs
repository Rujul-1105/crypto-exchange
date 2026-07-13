//! Per-user token bucket for rate limiting.
//!
//! Each user gets a bucket of `capacity` tokens. Tokens refill at
//! `refill_per_sec` per second. Each request consumes one token. If
//! the bucket is empty, the request is rejected.
//!
//! Phase 5 keeps this in-memory and per-process. The bucket state is
//! reset on restart (acceptable for a demo). A production system would
//! back this with Redis / a leaky-bucket DB row.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct BucketConfig {
    pub capacity: u32,
    pub refill_per_sec: u32,
}

pub struct RateLimiter {
    config: BucketConfig,
    inner: Mutex<HashMap<u64, Bucket>>,
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    pub fn new(config: BucketConfig) -> Self {
        RateLimiter {
            config,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Try to take one token for `user_id`. Returns true if allowed.
    pub fn try_take(&self, user_id: u64) -> bool {
        let Ok(mut guard) = self.inner.lock() else {
            return true; // fail-open: don't block on lock contention
        };
        let now = Instant::now();
        let bucket = guard.entry(user_id).or_insert_with(|| Bucket {
            tokens: self.config.capacity as f64,
            last_refill: now,
        });

        // Refill: tokens += (elapsed * refill_per_sec), capped at capacity.
        let elapsed = now.duration_since(bucket.last_refill);
        let refill =
            (elapsed.as_secs_f64() * self.config.refill_per_sec as f64).min(f64::MAX);
        bucket.tokens = (bucket.tokens + refill).min(self.config.capacity as f64);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Hard cap reset (admin tool / tests).
    #[allow(dead_code)]
    pub fn reset(&self, user_id: u64) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.remove(&user_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn burst_then_block() {
        let cfg = BucketConfig {
            capacity: 3,
            refill_per_sec: 0,
        };
        let limiter = RateLimiter::new(cfg);
        // First 3 succeed.
        assert!(limiter.try_take(1));
        assert!(limiter.try_take(1));
        assert!(limiter.try_take(1));
        // 4th fails — bucket empty.
        assert!(!limiter.try_take(1));
    }

    #[test]
    fn refill_caps_at_capacity() {
        let cfg = BucketConfig {
            capacity: 2,
            refill_per_sec: 1000,
        };
        let limiter = RateLimiter::new(cfg);
        assert!(limiter.try_take(1));
        assert!(limiter.try_take(1));
        assert!(!limiter.try_take(1));
        thread::sleep(Duration::from_millis(50));
        // 50ms * 1000/sec = 50 tokens (capped at capacity = 2).
        assert!(limiter.try_take(1));
        assert!(limiter.try_take(1));
        assert!(!limiter.try_take(1));
    }

    #[test]
    fn per_user_buckets() {
        let cfg = BucketConfig {
            capacity: 1,
            refill_per_sec: 0,
        };
        let limiter = RateLimiter::new(cfg);
        assert!(limiter.try_take(1));
        assert!(!limiter.try_take(1));
        // user 2 still has their own bucket.
        assert!(limiter.try_take(2));
        assert!(!limiter.try_take(2));
    }
}