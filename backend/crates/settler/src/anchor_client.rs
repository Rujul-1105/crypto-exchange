//! Anchor client wrapper — builds + submits `settle_fill` instructions.
//!
//! Settlement pipeline: `EngineEvent::Fill` → `SettlerClient::settle_fill`
//! → on-chain `settle_fill` ix → `SettleUpdate`. The retry+backoff loop in
//! `settle_fill` handles transient RPC errors (network blips, slot skips).

use common::*;
use rust_decimal::prelude::ToPrimitive;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signature, Signer},
    transaction::Transaction,
};
use std::time::Duration;
use tokio::time::sleep;

pub struct SettlerClient {
    rpc_client: RpcClient,
    keypair: Keypair,
    program_id: Pubkey,
    max_retries: u32,
}

impl SettlerClient {
    pub fn new(rpc_url: String, keypair: Keypair, program_id: Pubkey, max_retries: u32) -> Self {
        let rpc_client =
            RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
        Self {
            rpc_client,
            keypair,
            program_id,
            max_retries,
        }
    }

    /// Build a `SettleUpdate` for the given trade, retrying on failure.
    pub async fn settle_fill(&self, trade: &Trade) -> SettleUpdate {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.submit(trade).await {
                Ok(sig) => {
                    return SettleUpdate {
                        trade_id: trade.id,
                        symbol: trade.symbol.clone(),
                        status: SettleStatus::Confirmed,
                        signature: Some(sig.to_string()),
                        error: None,
                    };
                }
                Err(e) => {
                    tracing::warn!(
                        "settle_fill attempt {attempt} failed for trade_id={}: {e}",
                        trade.id
                    );
                    if attempt >= self.max_retries {
                        return SettleUpdate {
                            trade_id: trade.id,
                            symbol: trade.symbol.clone(),
                            status: SettleStatus::Failed,
                            signature: None,
                            error: Some(e.to_string()),
                        };
                    }
                    let delay = Duration::from_millis(200 * 2u64.pow(attempt.min(6)));
                    sleep(delay).await;
                }
            }
        }
    }

    /// Build the settle_fill instruction and submit it to the cluster.
    async fn submit(&self, trade: &Trade) -> anyhow::Result<Signature> {
        let ix = self.build_settle_fill_ix(trade)?;
        let recent = self.rpc_client.get_latest_blockhash().await?;
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&self.keypair.pubkey()),
            &[&self.keypair],
            recent,
        );
        let sig = self.rpc_client.send_and_confirm_transaction(&tx).await?;
        Ok(sig)
    }

    /// Build the `settle_fill` instruction:
    ///   accounts: settler(signer,mut), config, settlement_record(init),
    ///             buyer_balance(mut), seller_balance(mut), system_program
    ///   data:     discriminator(8) + borsh(buy_id, sell_id, price, qty)
    fn build_settle_fill_ix(&self, trade: &Trade) -> anyhow::Result<Instruction> {
        let program_id = self.program_id;

        // Derive PDAs. Note: `user_balance` seeds expect the raw 32-byte
        // pubkey (not the base58 ASCII string) — Anchor re-derives the PDA
        // from `buyer_balance.user.as_ref()` on-chain, so the client must
        // pass the same form.
        let (config_pda, _) = Pubkey::find_program_address(&[b"config"], &program_id);
        let (settlement_pda, _) = Pubkey::find_program_address(
            &[
                b"settlement",
                &trade.buy_order_id.to_le_bytes(),
                &trade.sell_order_id.to_le_bytes(),
            ],
            &program_id,
        );
        let buyer_pubkey: Pubkey = trade.buyer.parse()?;
        let seller_pubkey: Pubkey = trade.seller.parse()?;
        let (buyer_balance_pda, _) = Pubkey::find_program_address(
            &[b"user_balance", buyer_pubkey.as_ref()],
            &program_id,
        );
        let (seller_balance_pda, _) = Pubkey::find_program_address(
            &[b"user_balance", seller_pubkey.as_ref()],
            &program_id,
        );

        // Anchor account metas, in declared order.
        let accounts = vec![
            AccountMeta::new(self.keypair.pubkey(), true), // settler, signer, mut
            AccountMeta::new_readonly(config_pda, false),
            AccountMeta::new(settlement_pda, false), // init → mut
            AccountMeta::new(buyer_balance_pda, false), // mut
            AccountMeta::new(seller_balance_pda, false), // mut
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
        ];

        // 8-byte Anchor discriminator: first 8 bytes of `Sha256("global:settle_fill")`.
        let disc: [u8; 8] = solana_sdk::hash::hash(b"global:settle_fill").to_bytes()[..8]
            .try_into()
            .expect("sha256 output is 32 bytes");

        // Borsh-serialize args: (buy_order_id, sell_order_id, price, quantity).
        let price_u64: u64 = trade
            .price
            .to_u64()
            .ok_or_else(|| anyhow::anyhow!("price out of u64 range: {}", trade.price))?;
        let qty_u64: u64 = trade
            .quantity
            .to_u64()
            .ok_or_else(|| anyhow::anyhow!("quantity out of u64 range: {}", trade.quantity))?;

        let mut data = Vec::with_capacity(8 + 32);
        data.extend_from_slice(&disc);
        data.extend_from_slice(&trade.buy_order_id.to_le_bytes());
        data.extend_from_slice(&trade.sell_order_id.to_le_bytes());
        data.extend_from_slice(&price_u64.to_le_bytes());
        data.extend_from_slice(&qty_u64.to_le_bytes());

        Ok(Instruction {
            program_id,
            accounts,
            data,
        })
    }
}