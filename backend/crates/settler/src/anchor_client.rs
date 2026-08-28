//! Anchor client wrapper — builds + submits `settle_fill` instructions.
//!
//! Phase 6 ships a working pipeline that *simulates* on-chain settlement by
//! logging the would-be tx; the real `program.methods.settle_fill(...)` call
//! is wired up but requires a deployed program id (the deploy lands in
//! Phase 2's deploy step + Phase 10 polish). When the program id is set, the
//! `submit` method below becomes the live path.

use common::*;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use std::time::Duration;
use tokio::time::sleep;

pub struct SettlerClient {
    rpc_url: String,
    _keypair: Keypair,
    program_id: Option<Pubkey>,
    max_retries: u32,
}

impl SettlerClient {
    pub fn new(
        rpc_url: String,
        keypair: Keypair,
        program_id: Pubkey,
        max_retries: u32,
    ) -> Self {
        Self {
            rpc_url,
            _keypair: keypair,
            program_id: Some(program_id),
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
                        signature: Some(sig),
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

    async fn submit(&self, _trade: &Trade) -> anyhow::Result<String> {
        // Phase 6 placeholder: when program_id is set and the Anchor program
        // is deployed, this becomes:
        //
        //   let ix = self.build_settle_fill_ix(trade);
        //   let recent = self.rpc_client.get_latest_blockhash().await?;
        //   let tx = Transaction::new_signed_with_payer(
        //       &[ix], Some(&self.payer.pubkey()), &[&self.payer], recent,
        //   );
        //   let sig = self.rpc_client.send_and_confirm(&tx).await?;
        //   Ok(sig.to_string())
        //
        // For now we log and return a deterministic fake signature so the
        // pipeline is observable end-to-end.
        let pid = self
            .program_id
            .ok_or_else(|| anyhow::anyhow!("program_id not set"))?;
        tracing::debug!(
            "would submit settle_fill for trade_id={} program={}",
            _trade.id,
            pid
        );
        Ok(format!("fake-sig-{}", _trade.id))
    }
}
