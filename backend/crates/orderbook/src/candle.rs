//! Candle aggregator — turns a stream of trades into OHLCV buckets.
//!
//! Tracks multiple intervals (`1m`, `5m`, `1h`); each trade updates every
//! interval's bucket. The engine emits a `CandleUpdate` per trade for every
//! tracked interval (the API WS relay routes each one to the right
//! `candles:<sym>:<interval>` channel). Closed buckets are appended to a
//! bounded ring per interval so `GET /api/candles/:sym/:interval` can serve
//! historical candles.

use common::*;
use std::collections::{HashMap, VecDeque};

const TRACKED_INTERVALS: &[CandleInterval] =
    &[CandleInterval::M1, CandleInterval::M5, CandleInterval::H1];

/// Maximum closed candles retained per interval. Sized for the demo's REST
/// history endpoint (default `limit=500`) plus headroom for the chart's
/// in-memory store (ring cap 1000).
const HISTORY_CAP: usize = 1000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CandleAggregator {
    pub symbol: Symbol,
    buckets: HashMap<String, Candle>,
    /// Closed candles per interval (newest at back). Bounded by `HISTORY_CAP`.
    history: HashMap<String, VecDeque<Candle>>,
}

impl CandleAggregator {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            buckets: HashMap::new(),
            history: HashMap::new(),
        }
    }

    /// Update all buckets with this trade. Returns one `(interval, candle,
    /// is_closed)` entry per tracked interval. `is_closed=true` means the
    /// bucket just flipped and the returned `value` is the *previous*
    /// (now-closed) bucket; the new open bucket is in `self.buckets` but
    /// not in the tuple.
    pub fn update(
        &mut self,
        price: Price,
        qty: Quantity,
        ts: Timestamp,
    ) -> Vec<(CandleInterval, Candle, bool)> {
        let mut out = Vec::with_capacity(TRACKED_INTERVALS.len());

        for interval in TRACKED_INTERVALS {
            let bucket_ts = Candle::bucket_for(ts, *interval);
            let key = interval.as_str();
            let (returned, is_closed) = match self.buckets.get_mut(key) {
                Some(c) if c.open_ts == bucket_ts => {
                    c.update(price, qty);
                    (c.clone(), false)
                }
                Some(_) => {
                    // Bucket flipped: close the previous bucket, archive it,
                    // and start a fresh one with this trade.
                    let prev = self.buckets.remove(key).unwrap();
                    self.archive(*interval, prev.clone());
                    let mut new = Candle::new(self.symbol.clone(), *interval, bucket_ts, price);
                    new.update(price, qty);
                    self.buckets.insert(key.to_string(), new);
                    (prev, true)
                }
                None => {
                    let mut new = Candle::new(self.symbol.clone(), *interval, bucket_ts, price);
                    new.update(price, qty);
                    self.buckets.insert(key.to_string(), new.clone());
                    (new, true)
                }
            };
            out.push((*interval, returned, is_closed));
        }

        out
    }

    /// Snapshot the current open bucket for an interval, if any.
    pub fn snapshot(&self, interval: CandleInterval) -> Option<Candle> {
        self.buckets.get(interval.as_str()).cloned()
    }

    /// Closed-candle history for an interval (newest at back), oldest first.
    pub fn history(&self, interval: CandleInterval) -> Vec<Candle> {
        self.history
            .get(interval.as_str())
            .map(|d| d.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Closed-candle history followed by the current open bucket — what
    /// `/api/candles/:sym/:interval` should serve.
    pub fn history_with_open(&self, interval: CandleInterval) -> Vec<Candle> {
        let mut out = self.history(interval);
        if let Some(open) = self.snapshot(interval) {
            out.push(open);
        }
        out
    }

    /// Append a candle to an interval's ring, trimming from the front if over
    /// `HISTORY_CAP`.
    fn archive(&mut self, interval: CandleInterval, candle: Candle) {
        let key = interval.as_str();
        let ring = self.history.entry(key.to_string()).or_default();
        ring.push_back(candle);
        while ring.len() > HISTORY_CAP {
            ring.pop_front();
        }
    }
}