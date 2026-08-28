//! Solana CEX demo — vault-based token custody + settle_fill.
//!
//! Phase 2 implements: initialize, deposit_sol, deposit_usdc, withdraw_sol,
//! withdraw_usdc, settle_fill. Tests live in tests/.

use anchor_lang::prelude::*;

pub mod state;
pub mod errors;

pub use errors::*;
pub use state::*;

declare_id!("PLACEHOLDER_PROGRAM_ID");

#[program]
pub mod exchange {
    use super::*;

    /// One-time admin setup. Creates the `config` PDA and the `vault_authority` PDA
    /// that owns all user vault token accounts (and signs `settle_fill`).
    pub fn initialize(ctx: Context<Initialize>, fee_bps: u16) -> Result<()> {
        msg!("initialize: fee_bps={}", fee_bps);
        // TODO(phase-2): write config fields, vault_authority is created by `init`
        Ok(())
    }

    /// Wrap SOL → wSOL and deposit into the user's vault token account.
    pub fn deposit_sol(ctx: Context<DepositSol>, amount: u64) -> Result<()> {
        msg!("deposit_sol: amount={}", amount);
        // TODO(phase-2)
        Ok(())
    }

    /// Deposit USDC into the user's vault token account.
    pub fn deposit_usdc(ctx: Context<DepositUsdc>, amount: u64) -> Result<()> {
        msg!("deposit_usdc: amount={}", amount);
        // TODO(phase-2)
        Ok(())
    }

    /// Unwrap wSOL → SOL out of the user's vault token account to their wallet.
    pub fn withdraw_sol(ctx: Context<WithdrawSol>, amount: u64) -> Result<()> {
        msg!("withdraw_sol: amount={}", amount);
        // TODO(phase-2)
        Ok(())
    }

    /// Withdraw USDC from the user's vault token account to their ATA.
    pub fn withdraw_usdc(ctx: Context<WithdrawUsdc>, amount: u64) -> Result<()> {
        msg!("withdraw_usdc: amount={}", amount);
        // TODO(phase-2)
        Ok(())
    }

    /// Settle a matched trade. Called by the settler worker.
    /// Atomically transfers tokens between the two user vault token accounts.
    /// Idempotent on `(buy_order_id, sell_order_id)` for safe retries.
    pub fn settle_fill(
        ctx: Context<SettleFill>,
        buy_order_id: u64,
        sell_order_id: u64,
        price: u64,
        quantity: u64,
    ) -> Result<()> {
        msg!(
            "settle_fill: buy={} sell={} price={} qty={}",
            buy_order_id, sell_order_id, price, quantity
        );
        // TODO(phase-2)
        Ok(())
    }
}

// ── Account contexts (stubs; Phase 2 fills these in) ────────────────────

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
    /// CHECK: PDA used as authority for all vault token accounts.
    #[account(
        seeds = [b"vault_authority"],
        bump,
    )]
    pub vault_authority: UncheckedAccount<'info>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositSol<'info> {
    pub user: Signer<'info>,
    // TODO(phase-2): user_vault token account, vault_authority, system_program, token_program, wsol_mint, associated_token_program
}

#[derive(Accounts)]
pub struct DepositUsdc<'info> {
    pub user: Signer<'info>,
    // TODO(phase-2)
}

#[derive(Accounts)]
pub struct WithdrawSol<'info> {
    pub user: Signer<'info>,
    // TODO(phase-2)
}

#[derive(Accounts)]
pub struct WithdrawUsdc<'info> {
    pub user: Signer<'info>,
    // TODO(phase-2)
}

#[derive(Accounts)]
pub struct SettleFill<'info> {
    // TODO(phase-2): config, vault_authority (signer via PDA seeds),
    // maker_sol_vault, taker_sol_vault, maker_usdc_vault, taker_usdc_vault,
    // token_program, settlement_record PDA (idempotency on (buy_order_id, sell_order_id))
}
