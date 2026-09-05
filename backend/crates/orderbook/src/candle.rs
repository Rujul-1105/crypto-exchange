//! Candle aggregator — turns a stream of trades into OHLCV buckets.
//!
//! Tracks multiple intervals (`1m`, `5m`, `1h`); each trade updates every
//! interval's bucket. The engine emits a `CandleUpdate` per trade for the
//! `1m` bucket (which is what the chart consumes); the `5m` and `1h` buckets
//! are kept for REST `GET /api/candles` queries.

use common::*;
use std::collections::HashMap;

const TRACKED_INTERVALS: &[CandleInterval] =
    &[CandleInterval::M1, CandleInterval::M5, CandleInterval::H1];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CandleAggregator {
    pub symbol: Symbol,
    buckets: HashMap<String, Candle>,
}

impl CandleAggregator {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            buckets: HashMap::new(),
        }
    }

    /// Update all buckets with this trade. Returns the latest 1m candle and
    /// whether it just closed (the bucket flipped to a new minute).
    pub fn update(&mut self, price: Price, qty: Quantity, ts: Timestamp) -> (Candle, bool) {
        let mut one_min = None;

        for interval in TRACKED_INTERVALS {
            let bucket_ts = Candle::bucket_for(ts, *interval);
            let key = interval.as_str();
            let (candle, is_closed) = match self.buckets.get_mut(key) {
                Some(c) if c.open_ts == bucket_ts => {
                    c.update(price, qty);
                    (c.clone(), false)
                }
                Some(_) => {
                    // Close the previous bucket, return it as closed.
                    let prev = self.buckets.remove(key).unwrap();
                    let mut new = Candle::new(self.symbol.clone(), *interval, bucket_ts, price);
                    new.update(price, qty);
                    self.buckets.insert(key.to_string(), new.clone());
                    (prev, true)
                }
                None => {
                    let mut new = Candle::new(self.symbol.clone(), *interval, bucket_ts, price);
                    new.update(price, qty);
                    self.buckets.insert(key.to_string(), new.clone());
                    (new, true)
                }
            };
            if *interval == CandleInterval::M1 {
                one_min = Some((candle, is_closed));
            }
        }
        one_min.unwrap_or_else(|| {
            (
                Candle::new(self.symbol.clone(), CandleInterval::M1, ts, price),
                true,
            )
        })
    }

    /// Snapshot all current buckets. Used by `GET /api/candles/:sym/:interval`.
    pub fn snapshot(&self, interval: CandleInterval) -> Option<Candle> {
        self.buckets.get(interval.as_str()).cloned()
    }
}
