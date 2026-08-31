//! Anchor account-context structs for every instruction in the program.
//!
//! Each struct is the * typed inventory of "which accounts does this instruction
//! need to touch?" that gets passed via `Context<T>` into the handler. The
//! `#[derive(Accounts)]` macro generates a `try_accounts` method that, at
//! instruction entry, validates every field against its declared constraints
//! and deserializes the data.
//!
//! Module declaration lives in `lib.rs` next to `mod errors` / `mod state`,
//! and items are re-exported at the crate root via `pub use instructions::*`,
//! so handlers inside `#[program] mod exchange { … }` can keep writing
//! `Context<Initialize>`, `Context<DepositSol>`, etc. without a prefix.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::state::{Config, SettlementRecord, UserBalance};

// ── Admin ───────────────────────────────────────────────

/// One-time admin setup. Creates the `config` and `vault_authority` PDAs.
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = admin,
        space = Config::SIZE,
        seeds = [b"config"],
        bump,
    )]
    pub config: Account<'info, Config>,

    /// CHECK: PDA owning the shared vault token accounts. Created here by `init`.
    #[account(
        seeds = [b"vault_authority"],
        bump,
    )]
    pub vault_authority: AccountInfo<'info>,

    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

/// Pause-switch gate. `has_one = admin` couples the supplied signer to
/// `config.admin`, so only the recorded admin can flip the `paused` flag.
#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(
        seeds = [b"config"],
        bump = config.bump,
        has_one = admin,
    )]
    pub config: Account<'info, Config>,

    pub admin: Signer<'info>,
}

// ── Deposits ────────────────────────────────────────────

/// Deposit wSOL into the user's available SOL balance.
///
/// Transfers wSOL from the user's wSOL ATA into the shared SOL vault. First
/// deposit creates both the shared vault and the user's wSOL ATA via
/// `init_if_needed`.
#[derive(Accounts)]
pub struct DepositSol<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,

    /// CHECK: PDA owning the shared vault token accounts.
    #[account(seeds = [b"vault_authority"], bump)]
    pub vault_authority: AccountInfo<'info>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = wsol_mint,
        associated_token::authority = vault_authority,
    )]
    pub shared_sol_vault: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = wsol_mint,
        associated_token::authority = user,
    )]
    pub user_wsol_ata: Account<'info, TokenAccount>,

    // user ledger
    #[account(
        init_if_needed,
        payer = user,
        space = UserBalance::SIZE,
        seeds = [b"user_balance", user.key().as_ref()],
        bump,
    )]
    pub user_balance: Account<'info, UserBalance>,

    pub wsol_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Deposit USDC into the user's available USDC balance. Mirror of `DepositSol`
/// against the USDC mint.
#[derive(Accounts)]
pub struct DepositUsdc<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,

    /// CHECK: PDA
    #[account(seeds = [b"vault_authority"], bump)]
    pub vault_authority: AccountInfo<'info>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = usdc_mint,
        associated_token::authority = vault_authority,
    )]
    pub shared_usdc_vault: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = usdc_mint,
        associated_token::authority = user,
    )]
    pub user_usdc_ata: Account<'info, TokenAccount>,

    // user ledger
    #[account(
        init_if_needed,
        payer = user,
        space = UserBalance::SIZE,
        seeds = [b"user_balance", user.key().as_ref()],
        bump,
    )]
    pub user_balance: Account<'info, UserBalance>,

    pub usdc_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

// ── Withdrawals ─────────────────────────────────────────

/// Withdraw wSOL from the user's available SOL balance to their wSOL ATA.
/// `shared_sol_vault` must already exist (no `init_if_needed`); the user's
/// wSOL ATA may be created lazily.
#[derive(Accounts)]
pub struct WithdrawSol<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,

    /// CHECK: PDA, signer via seeds
    #[account(seeds = [b"vault_authority"], bump)]
    pub vault_authority: AccountInfo<'info>,

    #[account(
        mut,
        associated_token::mint = wsol_mint,
        associated_token::authority = vault_authority,
    )]
    pub shared_sol_vault: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = wsol_mint,
        associated_token::authority = user,
    )]
    pub user_wsol_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"user_balance", user.key().as_ref()],
        bump = user_balance.bump,
    )]
    pub user_balance: Account<'info, UserBalance>,

    pub wsol_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Withdraw USDC from the user's available USDC balance to their USDC ATA.
/// Mirror of `WithdrawSol` against the USDC mint.
#[derive(Accounts)]
pub struct WithdrawUsdc<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,

    /// CHECK: PDA, signer via seeds
    #[account(seeds = [b"vault_authority"], bump)]
    pub vault_authority: AccountInfo<'info>,

    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = vault_authority,
    )]
    pub shared_usdc_vault: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = usdc_mint,
        associated_token::authority = user,
    )]
    pub user_usdc_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"user_balance", user.key().as_ref()],
        bump = user_balance.bump,
    )]
    pub user_balance: Account<'info, UserBalance>,

    pub usdc_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

// ── Settlement ──────────────────────────────────────────

/// Settle a matched trade. Idempotent on `(buy_order_id, sell_order_id)`.
///
/// The seeds of `settlement_record` include `buy_order_id` and
/// `sell_order_id`, exposed to the seed list via the `#[instruction(...)]`
/// attribute. `init` (not `init_if_needed`) is what makes the instruction
/// idempotent — a duplicate settle fails at the runtime because the
/// `settlement_record` account already exists at the same PDA.
///
/// `buyer_balance` and `seller_balance` are passed by pubkey; Anchor
/// re-derives each PDA from the declared seed list. The seed expression
/// `buyer_balance.user.as_ref()` reads the buyer pubkey out of the
/// deserialized `UserBalance` — this works because Anchor validates accounts
/// in declared order, so `buyer_balance` is deserialized before
/// `seller_balance`'s seeds are evaluated.
#[derive(Accounts)]
#[instruction(buy_order_id: u64, sell_order_id: u64, _price: u64, _quantity: u64)]
pub struct SettleFill<'info> {
    /// Any signer (settler worker); the instruction is permissionless and
    /// idempotent on (buy_order_id, sell_order_id).
    #[account(mut)]
    pub settler: Signer<'info>,

    #[account(seeds = [b"config"], bump = config.bump)]
    pub config: Account<'info, Config>,

    #[account(
        init,
        payer = settler,
        space = SettlementRecord::SIZE,
        seeds = [
            b"settlement",
            buy_order_id.to_le_bytes().as_ref(),
            sell_order_id.to_le_bytes().as_ref(),
        ],
        bump,
    )]
    pub settlement_record: Account<'info, SettlementRecord>,

    #[account(
        mut,
        seeds = [b"user_balance", buyer_balance.user.as_ref()],
        bump = buyer_balance.bump,
    )]
    pub buyer_balance: Account<'info, UserBalance>,

    #[account(
        mut,
        seeds = [b"user_balance", seller_balance.user.as_ref()],
        bump = seller_balance.bump,
    )]
    pub seller_balance: Account<'info, UserBalance>,

    pub system_program: Program<'info, System>,
}