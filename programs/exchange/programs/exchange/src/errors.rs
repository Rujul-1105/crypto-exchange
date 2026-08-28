//! Custom error codes for the exchange program.

use anchor_lang::prelude::*;

#[error_code]
pub enum ExchangeError {
    #[msg("Settlement record already exists for this order pair")]
    SettlementAlreadyExists,
    #[msg("Order IDs are not distinct (self-trade not allowed via settle_fill)")]
    SameOrderIds,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Vault has insufficient balance")]
    InsufficientVaultBalance,
    #[msg("Exchange is paused")]
    ExchangePaused,
    #[msg("Provided PDA does not match the expected vault authority")]
    InvalidVaultAuthority,
}
