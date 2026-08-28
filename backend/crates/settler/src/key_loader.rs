//! Load a keypair from a Solana CLI JSON file (`~/.config/solana/id.json`).
//!
//! If the file is missing, we generate a fresh keypair and write it to the
//! target path so dev workflows don't have to manually create one.

use solana_sdk::signature::{Keypair, Signer};
use std::path::Path;

pub fn load_keypair(path: &Path) -> anyhow::Result<Keypair> {
    if path.exists() {
        let bytes = std::fs::read(path)?;
        let keypair = Keypair::try_from(bytes.as_slice())
            .map_err(|e| anyhow::anyhow!("parse keypair {}: {e}", path.display()))?;
        return Ok(keypair);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let keypair = Keypair::new();
    std::fs::write(path, keypair.to_bytes().to_vec())?;
    tracing::warn!(
        "generated new settler keypair at {} (pubkey={})",
        path.display(),
        keypair.pubkey()
    );
    Ok(keypair)
}
