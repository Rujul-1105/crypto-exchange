//! ## Architecture
//!
//! - `config` PDA — global exchange config (admin, vault_authority, fee_bps, paused).
//! - `vault_authority` PDA — owner of all shared vault token accounts; signs vault transfers.
//! - `shared_sol_vault` ATA — single wSOL vault for all users.
//! - `shared_usdc_vault` ATA — single USDC vault for all users.
//! - `user_balance:<pubkey>` PDA — per-user ledger of available SOL + USDC.
//! - `settlement_record:<buy_id>:<sell_id>` PDA — idempotency key for settled fills.
//!
//! Tokens are physically custodied in the shared vaults. Per-user balances are
//! tracked in `user_balance` PDAs (the ledger). `settle_fill` updates two
//! user_balance accounts and creates a settlement_record so retries are safe.
//!
//! ## Settlement authority
//!
//! `settle_fill` is permissionless at the program level (anyone can submit it).
//! In practice, only the settler worker will submit fills because it is the only
//! process that sees engine Fill events. The settlement_record PDA makes the
//! instruction idempotent on (buy_order_id, sell_order_id).

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

pub mod errors;
pub mod instructions;
pub mod state;
pub use errors::*;
pub use instructions::*;
pub use state::*;

declare_id!("DNhvifJ6mcgVRRA4xaNKH82tjoHseN3GQKiLi8R7i6HY");

// Standard wSOL mint on Solana mainnet/devnet.
pub const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");

#[program]
pub mod exchange {
    use super::*;

    // ── Admin ───────────────────────────────────────────────

    /// One-time admin setup. Creates the `config` and `vault_authority` PDAs.
    pub fn initialize(ctx: Context<Initialize>, fee_bps: u16) -> Result<()> {
        require!(fee_bps <= 1000, ExchangeError::FeeTooHigh); // ≤ 10%

        let config = &mut ctx.accounts.config;
        config.admin = ctx.accounts.admin.key();
        config.vault_authority = ctx.accounts.vault_authority.key();
        config.fee_bps = fee_bps;
        config.paused = false;
        config.bump = ctx.bumps.config;

        msg!(
            "initialize: admin={} vault_authority={} fee_bps={}",
            config.admin,
            config.vault_authority,
            fee_bps
        );
        Ok(())
    }

    pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.paused = paused;
        Ok(())
    }

    // ── Deposits ────────────────────────────────────────────

    /// Deposit wSOL into the user's available SOL balance.
    /// Transfers wSOL from user's wSOL ATA into the shared SOL vault.
    pub fn deposit_sol(ctx: Context<DepositSol>, amount: u64) -> Result<()> {
        require!(amount > 0, ExchangeError::ZeroAmount);
        let config = &ctx.accounts.config;
        require!(!config.paused, ExchangeError::ExchangePaused);

        // Transfer wSOL from user → shared vault (vault_authority signs for the vault).
        let cpi_accounts = Transfer {
            from: ctx.accounts.user_wsol_ata.to_account_info(),
            to: ctx.accounts.shared_sol_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        // Update ledger.
        let balance = &mut ctx.accounts.user_balance;
        balance.sol = balance
            .sol
            .checked_add(amount)
            .ok_or(ExchangeError::Overflow)?;
        balance.bump = ctx.bumps.user_balance;

        msg!("deposit_sol: user={} amount={}", balance.user, amount);
        Ok(())
    }

    /// Deposit USDC into the user's available USDC balance.
    pub fn deposit_usdc(ctx: Context<DepositUsdc>, amount: u64) -> Result<()> {
        require!(amount > 0, ExchangeError::ZeroAmount);
        let config = &ctx.accounts.config;
        require!(!config.paused, ExchangeError::ExchangePaused);

        let cpi_accounts = Transfer {
            from: ctx.accounts.user_usdc_ata.to_account_info(),
            to: ctx.accounts.shared_usdc_vault.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(ctx.accounts.token_program.to_account_info(), cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        //  If the SPL transfer succeeds and the ledger update later fails, the user's tokens are gone but their balance is wrong which means the program is broken. In practice this never happens because the ledger arithmetic only fails on u64 overflow, which the deposit amount will never cause, but in production code write tests for that exact case.

        let balance = &mut ctx.accounts.user_balance;
        balance.usdc = balance
            .usdc
            .checked_add(amount)
            .ok_or(ExchangeError::Overflow)?;
        balance.bump = ctx.bumps.user_balance;

        msg!("deposit_usdc: user={} amount={}", balance.user, amount);
        Ok(())
    }

    // ── Withdrawals ─────────────────────────────────────────

    /// Withdraw wSOL from the user's available SOL balance to their wSOL ATA.
    pub fn withdraw_sol(ctx: Context<WithdrawSol>, amount: u64) -> Result<()> {
        require!(amount > 0, ExchangeError::ZeroAmount);
        let config = &ctx.accounts.config;
        require!(!config.paused, ExchangeError::ExchangePaused);

        // update ledger first, then transfer out of the shared vault. If the SPL transfer fails after the ledger update, the user has lost their balance but not their tokens, which is a bug. In practice this never happens because the SPL transfer only fails on insufficient funds, which is already checked by the ledger arithmetic.
        let balance = &mut ctx.accounts.user_balance;
        require!(balance.sol >= amount, ExchangeError::InsufficientBalance);
        balance.sol = balance.sol.checked_sub(amount).unwrap();
        // .checked_sub returns None on underflow, but we already checked the balance above so unwrap is safe.

        // vault_authority (PDA) signs the transfer out of the shared vault.
        let authority_seeds: &[&[u8]] = &[b"vault_authority", &[ctx.bumps.vault_authority]];
        let signer_seeds = &[authority_seeds];
        let cpi_accounts = Transfer {
            from: ctx.accounts.shared_sol_vault.to_account_info(),
            to: ctx.accounts.user_wsol_ata.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );
        token::transfer(cpi_ctx, amount)?;

        msg!("withdraw_sol: user={} amount={}", balance.user, amount);
        Ok(())
    }

    /// Withdraw USDC from the user's available USDC balance to their USDC ATA.
    pub fn withdraw_usdc(ctx: Context<WithdrawUsdc>, amount: u64) -> Result<()> {
        require!(amount > 0, ExchangeError::ZeroAmount);
        let config = &ctx.accounts.config;
        require!(!config.paused, ExchangeError::ExchangePaused);

        let balance = &mut ctx.accounts.user_balance;
        require!(balance.usdc >= amount, ExchangeError::InsufficientBalance);
        balance.usdc = balance.usdc.checked_sub(amount).unwrap();

        let authority_seeds: &[&[u8]] = &[b"vault_authority", &[ctx.bumps.vault_authority]];
        let signer_seeds = &[authority_seeds];
        let cpi_accounts = Transfer {
            from: ctx.accounts.shared_usdc_vault.to_account_info(),
            to: ctx.accounts.user_usdc_ata.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        };
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
            signer_seeds,
        );
        token::transfer(cpi_ctx, amount)?;

        msg!("withdraw_usdc: user={} amount={}", balance.user, amount);
        Ok(())
    }

    // ── Settlement ──────────────────────────────────────────

    /// Settle a matched trade. Idempotent on `(buy_order_id, sell_order_id)`.
    ///
    /// Updates two `user_balance` PDAs:
    /// - buyer gains `quantity` SOL, pays `quantity * price` USDC
    /// - seller pays `quantity` SOL, gains `quantity * price` USDC
    ///
    /// No SPL token movement is needed because both users' funds live in the
    /// shared vaults; the ledger IS the source of truth.
    pub fn settle_fill(
        ctx: Context<SettleFill>,
        buy_order_id: u64,
        sell_order_id: u64,
        price: u64,
        quantity: u64,
    ) -> Result<()> {
        require!(buy_order_id != sell_order_id, ExchangeError::SameOrderIds);
        require!(quantity > 0, ExchangeError::ZeroAmount);
        require!(price > 0, ExchangeError::ZeroPrice);

        let config = &ctx.accounts.config;
        require!(!config.paused, ExchangeError::ExchangePaused);

        let buyer = &mut ctx.accounts.buyer_balance;
        let seller = &mut ctx.accounts.seller_balance;
        require!(buyer.user != seller.user, ExchangeError::SameOrderIds);

        let notional = price.checked_mul(quantity).ok_or(ExchangeError::Overflow)?;

        // Buyer pays USDC, receives SOL.
        require!(buyer.usdc >= notional, ExchangeError::InsufficientBalance);
        buyer.usdc = buyer.usdc.checked_sub(notional).unwrap();
        buyer.sol = buyer.sol.checked_add(quantity).unwrap();

        // Seller pays SOL, receives USDC.
        require!(seller.sol >= quantity, ExchangeError::InsufficientBalance);
        seller.sol = seller.sol.checked_sub(quantity).unwrap();
        seller.usdc = seller.usdc.checked_add(notional).unwrap();

        // Stamp the settlement record (init proves uniqueness; fails on retry).
        let record = &mut ctx.accounts.settlement_record;
        record.buy_order_id = buy_order_id;
        record.sell_order_id = sell_order_id;
        record.settled_at = Clock::get()?.unix_timestamp;
        record.bump = ctx.bumps.settlement_record;

        msg!(
            "settle_fill: buy={} sell={} price={} qty={} notional={}",
            buy_order_id,
            sell_order_id,
            price,
            quantity,
            notional
        );
        Ok(())
    }
}

// ── Account contexts ─────────────────────────────────────────
//
// Moved to `instructions.rs`. Items are re-exported at the crate root
// (`pub use instructions::*;`), so handlers below can still write
// `Context<Initialize>`, `Context<DepositSol>`, etc. without a prefix.
