//! API server (actix-web). Phase 5 fills this in with REST + WS + SIWS/JWT auth.

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("api server placeholder (Phase 5 will implement REST + WS)");
    Ok(())
}
