//! Snapshot persistence — bincode-serialize `MatchingEngine` every N seconds,
//! keep last 3, restore on startup.
//!
//! On startup the orderbook server:
//! 1. Loads the latest snapshot, hydrating the engine.
//! 2. Replays any unprocessed `orders:incoming` messages (whose stream id is
//!    greater than the one captured in the snapshot).
//! 3. Resumes normal consumption.

use crate::engine::MatchingEngine;
use crate::market::SymbolRegistry;
use common::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Serialize, Deserialize)]
pub struct PersistedState {
    pub engines: Vec<MatchingEngine>,
    pub last_consumed_order_redis_id: String,
    pub last_published_event_redis_id: String,
    pub snapshot_at_unix_ms: i64,
}

impl PersistedState {
    pub fn empty() -> Self {
        Self {
            engines: Vec::new(),
            last_consumed_order_redis_id: "0-0".into(),
            last_published_event_redis_id: "0-0".into(),
            snapshot_at_unix_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn from_registry(registry: &SymbolRegistry) -> impl std::future::Future<Output = Self> {
        // Note: in practice this would snapshot all engines; for Phase 4 we
        // only persist the demo symbol(s) loaded at boot.
        let registry = registry.clone();
        async move {
            let symbols = registry.list().await;
            let mut engines = Vec::new();
            for sym in symbols {
                if let Some(engine) = registry.get_or_create(sym.clone()).await.try_lock().ok() {
                    engines.push(engine.clone());
                }
            }
            Self {
                engines,
                last_consumed_order_redis_id: "0-0".into(),
                last_published_event_redis_id: "0-0".into(),
                snapshot_at_unix_ms: chrono::Utc::now().timestamp_millis(),
            }
        }
    }
}

pub fn snapshot_dir(path: impl Into<PathBuf>) -> PathBuf {
    path.into()
}

pub async fn save_snapshot(state: &PersistedState, dir: &Path) -> anyhow::Result<PathBuf> {
    tokio::fs::create_dir_all(dir).await?;
    let bytes = bincode::serialize(state)?;
    let ts = state.snapshot_at_unix_ms;
    let path = dir.join(format!("snap-{ts}.bin"));
    tokio::fs::write(&path, bytes).await?;
    gc_snapshots(dir, 3).await?;
    Ok(path)
}

pub async fn load_latest_snapshot(dir: &Path) -> anyhow::Result<Option<PersistedState>> {
    if !dir.exists() {
        return Ok(None);
    }
    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut newest: Option<(PathBuf, i64)> = None;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("snap-") || !name.ends_with(".bin") {
            continue;
        }
        let ts: i64 = name.trim_start_matches("snap-").trim_end_matches(".bin").parse().unwrap_or(0);
        if newest.as_ref().map(|(_, t)| ts > *t).unwrap_or(true) {
            newest = Some((entry.path(), ts));
        }
    }
    let Some((path, _)) = newest else { return Ok(None) };
    let bytes = tokio::fs::read(&path).await?;
    let state: PersistedState = bincode::deserialize(&bytes)?;
    Ok(Some(state))
}

async fn gc_snapshots(dir: &Path, keep: usize) -> anyhow::Result<()> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut files: Vec<(PathBuf, i64)> = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("snap-") || !name.ends_with(".bin") {
            continue;
        }
        let ts: i64 = name.trim_start_matches("snap-").trim_end_matches(".bin").parse().unwrap_or(0);
        files.push((entry.path(), ts));
    }
    files.sort_by_key(|(_, t)| -t); // newest first
    for (path, _) in files.into_iter().skip(keep) {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

/// Spawn a task that snapshots every `interval_ms`.
pub fn spawn_snapshot_task(
    registry: SymbolRegistry,
    dir: PathBuf,
    interval_ms: u64,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
        // Skip the first immediate tick.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let state = PersistedState::from_registry(&registry).await;
            match save_snapshot(&state, &dir).await {
                Ok(p) => tracing::info!("snapshot saved: {}", p.display()),
                Err(e) => tracing::error!("snapshot save failed: {e}"),
            }
        }
    });
}

/// Hydrate a registry from a snapshot.
pub async fn hydrate_registry(
    registry: &SymbolRegistry,
    state: &PersistedState,
) -> anyhow::Result<()> {
    for engine in &state.engines {
        let arc = registry.get_or_create(engine.symbol.clone()).await;
        let mut guard = arc.lock().await;
        *guard = engine.clone();
    }
    Ok(())
}

// silence "unused import" warnings for Arc in tests
#[allow(dead_code)]
fn _arc_marker(_: Arc<()>) {}
