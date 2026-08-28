//! Orderbook server (actix-web admin + matching engine). Phase 4 fills this in.

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("orderbook server placeholder (Phase 4 will implement matching engine + Redis consumer)");
    Ok(())
}
