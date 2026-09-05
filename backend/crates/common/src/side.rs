//! Side, OrderType, TimeInForce, OrderStatus, SettleStatus.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

pub type Price = Decimal;
pub type Quantity = Decimal;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Side {
    #[serde(rename = "buy")]
    Buy,
    #[serde(rename = "sell")]
    Sell,
}

impl Side {
    pub fn opposite(&self) -> Side {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrderType {
    Limit,
    Market,
    /// Stop-loss that becomes a market order when `trigger` price is crossed.
    Stop {
        trigger: Price,
    },
    /// Stop that becomes a limit at `limit` when `trigger` is crossed.
    StopLimit {
        trigger: Price,
        limit: Price,
    },
}

impl OrderType {
    pub fn is_stop(&self) -> bool {
        matches!(self, OrderType::Stop { .. } | OrderType::StopLimit { .. })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeInForce {
    /// Good-till-cancel — rests on the book until cancelled or filled.
    Gtc,
    /// Immediate-or-cancel — fill what you can, drop the rest.
    Ioc,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SettleStatus {
    #[default]
    Pending,
    Confirmed,
    Failed,
}
