//! On-chain account layouts for the exchange program.

use anchor_lang::prelude::*;

#[account]
pub struct Config {
    pub admin: Pubkey,           // 32
    pub vault_authority: Pubkey, // 32
    pub fee_bps: u16,            // 2
    pub paused: bool,            // 1
    pub bump: u8,                // 1
    pub _padding: [u8; 64],      // reserve
}

impl Config {
    pub const SIZE: usize = 8 + 32 + 32 + 2 + 1 + 1 + 64;
}

/// Per-user ledger entry tracking available (unsettled) SOL + USDC balances.
/// Owned by `user` (the wallet whose pubkey is the second seed). The matching
/// engine authorises updates by passing `user` as an account to `settle_fill`.
#[account]
pub struct UserBalance {
    pub user: Pubkey, // 32
    pub sol: u64,     // 8
    pub usdc: u64,    // 8
    pub bump: u8,     // 1
    pub _padding: [u8; 7],
}

impl UserBalance {
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 1 + 7;
}

/// Idempotency record for a settled fill — PDA keyed by
/// `("settlement", buy_order_id, sell_order_id)`. `init` on the account
/// guarantees uniqueness; retrying the same fill fails on the second attempt.
#[account]
pub struct SettlementRecord {
    pub buy_order_id: u64,  // 8
    pub sell_order_id: u64, // 8
    pub settled_at: i64,    // 8
    pub bump: u8,           // 1
    pub _padding: [u8; 7],
}

impl SettlementRecord {
    pub const SIZE: usize = 8 + 8 + 8 + 8 + 1 + 7;
}
