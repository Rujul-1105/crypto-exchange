//! Custom error codes for the exchange program.

use anchor_lang::prelude::*;

#[error_code]
pub enum ExchangeError {
    #[msg("Order IDs are not distinct (self-trade not allowed via settle_fill)")]
    SameOrderIds,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Price must be greater than zero")]
    ZeroPrice,
    #[msg("User has insufficient balance")]
    InsufficientBalance,
    #[msg("Exchange is paused")]
    ExchangePaused,
    #[msg("Provided PDA does not match the expected vault authority")]
    InvalidVaultAuthority,
    #[msg("Arithmetic overflow")]
    Overflow,
    #[msg("Fee bps exceeds 10% (1000)")]
    FeeTooHigh,
}
