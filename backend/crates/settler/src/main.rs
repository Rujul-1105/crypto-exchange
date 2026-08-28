//! Settler worker — consumes Fill events from `events:outgoing`, submits the
//! `settle_fill` Anchor instruction, and publishes status updates on
//! `settle:updates`.

mod anchor_client;
mod fill_consumer;
mod key_loader;

use anchor_client::SettlerClient;
use common::*;
use fill_consumer::FillConsumer;
use key_loader::load_keypair;
use redis::aio::ConnectionManager;
use std::path::PathBuf;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,settler=debug")),
        )
        .init();

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());
    let rpc_url = std::env::var("SETTLER_RPC_URL")
        .unwrap_or_else(|_| "https://api.devnet.solana.com".into());
    let keypair_path: PathBuf = std::env::var("SETTLER_KEYPAIR_PATH")
        .unwrap_or_else(|_| "./keys/settler.json".into())
        .into();
    let program_id_str = std::env::var("SETTLER_PROGRAM_ID")
        .unwrap_or_default();
    let max_retries: u32 = std::env::var("SETTLER_MAX_RETRIES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    tracing::info!("settler starting (rpc={} program={})", rpc_url, program_id_str);

    let keypair = load_keypair(&keypair_path)?;
    let program_id = parse_pubkey(&program_id_str)?;

    let client = redis::Client::open(redis_url.clone())?;
    let conn = ConnectionManager::new(client).await?;

    let settler = SettlerClient::new(rpc_url, keypair, program_id, max_retries);
    let consumer = FillConsumer::new(conn, settler);

    // Reconnect loop.
    loop {
        if let Err(e) = consumer.run().await {
            tracing::error!("consumer error: {e}; restarting in 1s");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

fn parse_pubkey(s: &str) -> anyhow::Result<solana_sdk::pubkey::Pubkey> {
    if s.is_empty() {
        anyhow::bail!("SETTLER_PROGRAM_ID not set; deploy the Anchor program first");
    }
    Ok(s.parse()?)
}
