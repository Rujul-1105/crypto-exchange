//! Settler worker (consumes Fill events, submits Anchor settle_fill). Phase 6 fills this in.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("settler worker placeholder (Phase 6 will implement Anchor settle_fill)");
    Ok(())
}
