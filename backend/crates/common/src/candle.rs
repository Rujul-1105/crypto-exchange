//! Candle (OHLCV bucket) emitted on every trade.

use crate::order::*;
use crate::side::*;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandleInterval {
    M1,
    M5,
    M15,
    H1,
    H4,
    D1,
}

impl CandleInterval {
    pub fn as_str(&self) -> &'static str {
        match self {
            CandleInterval::M1 => "1m",
            CandleInterval::M5 => "5m",
            CandleInterval::M15 => "15m",
            CandleInterval::H1 => "1h",
            CandleInterval::H4 => "4h",
            CandleInterval::D1 => "1d",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "1m" => Some(CandleInterval::M1),
            "5m" => Some(CandleInterval::M5),
            "15m" => Some(CandleInterval::M15),
            "1h" => Some(CandleInterval::H1),
            "4h" => Some(CandleInterval::H4),
            "1d" => Some(CandleInterval::D1),
            _ => None,
        }
    }

    pub fn millis(&self) -> i64 {
        match self {
            CandleInterval::M1 => 60_000,
            CandleInterval::M5 => 5 * 60_000,
            CandleInterval::M15 => 15 * 60_000,
            CandleInterval::H1 => 60 * 60_000,
            CandleInterval::H4 => 4 * 60 * 60_000,
            CandleInterval::D1 => 24 * 60 * 60_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub symbol: Symbol,
    pub interval: String,
    pub open_ts: Timestamp,
    pub close_ts: Timestamp,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Quantity,
}

impl Candle {
    pub fn new(
        symbol: Symbol,
        interval: CandleInterval,
        bucket_ts: Timestamp,
        price: Price,
    ) -> Self {
        Self {
            symbol,
            interval: interval.as_str().to_string(),
            open_ts: bucket_ts,
            close_ts: bucket_ts + interval.millis(),
            open: price,
            high: price,
            low: price,
            close: price,
            volume: Quantity::ZERO,
        }
    }

    pub fn update(&mut self, price: Price, qty: Quantity) {
        if price > self.high {
            self.high = price;
        }
        if price < self.low {
            self.low = price;
        }
        self.close = price;
        self.volume += qty;
    }

    pub fn bucket_for(ts: Timestamp, interval: CandleInterval) -> Timestamp {
        let bucket = interval.millis();
        (ts / bucket) * bucket
    }
}
