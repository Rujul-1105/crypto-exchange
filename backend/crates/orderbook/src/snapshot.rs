//! Snapshot persistence — bincode-serialize `MatchingEngine` every N seconds,
//! keep last 3, restore on startup.
//!
//! On startup the orderbook server:
//! 1. Loads the latest snapshot, hydrating the engine.
//! 2. Replays any unprocessed `orders:incoming` messages (whose stream id is
//!    greater than the one captured in the snapshot — passed in via the
//!    `Cursor` shared with the consumer).
//! 3. Resumes normal consumption.

use crate::engine::MatchingEngine;
use crate::market::SymbolRegistry;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Shared cursor for the last consumed `orders:incoming` stream id. Updated
/// by `spawn_consumer` and read by `spawn_snapshot_task` so the snapshot
/// persists the live cursor, not a stale `"0-0"`.
pub type Cursor = Arc<Mutex<String>>;

pub fn new_cursor(initial: impl Into<String>) -> Cursor {
    Arc::new(Mutex::new(initial.into()))
}

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

    pub fn from_registry(
        registry: &SymbolRegistry,
        cursor: &Cursor,
    ) -> impl std::future::Future<Output = Self> {
        let registry = registry.clone();
        let cursor = cursor.clone();
        async move {
            let symbols = registry.list().await;
            let mut engines = Vec::new();
            for sym in symbols {
                if let Some(engine) = registry.get_or_create(sym.clone()).await.try_lock().ok() {
                    engines.push(engine.clone());
                }
            }
            // Read the live cursor so the snapshot captures exactly where
            // the consumer is, not a stale placeholder.
            let last_consumed = cursor
                .lock()
                .map(|c| c.clone())
                .unwrap_or_else(|_| "0-0".into());
            Self {
                engines,
                last_consumed_order_redis_id: last_consumed,
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
    // Serialize as JSON instead of bincode. bincode 1.x rejects BTreeMap
    // keys whose serde impl calls `serialize_str` (rust_decimal::Decimal),
    // because bincode needs a size hint up front for sequences/maps with
    // string-like keys. JSON has no such restriction.
    let bytes = serde_json::to_vec(state)?;
    let ts = state.snapshot_at_unix_ms;
    let path = dir.join(format!("snap-{ts}.json"));
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
        if !name.starts_with("snap-") || !name.ends_with(".json") {
            continue;
        }
        let ts: i64 = name
            .trim_start_matches("snap-")
            .trim_end_matches(".json")
            .parse()
            .unwrap_or(0);
        if newest.as_ref().map(|(_, t)| ts > *t).unwrap_or(true) {
            newest = Some((entry.path(), ts));
        }
    }
    let Some((path, _)) = newest else {
        return Ok(None);
    };
    let bytes = tokio::fs::read(&path).await?;
    let state: PersistedState = serde_json::from_slice(&bytes)?;
    Ok(Some(state))
}

async fn gc_snapshots(dir: &Path, keep: usize) -> anyhow::Result<()> {
    let mut entries = tokio::fs::read_dir(dir).await?;
    let mut files: Vec<(PathBuf, i64)> = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with("snap-") || !name.ends_with(".json") {
            continue;
        }
        let ts: i64 = name
            .trim_start_matches("snap-")
            .trim_end_matches(".json")
            .parse()
            .unwrap_or(0);
        files.push((entry.path(), ts));
    }
    files.sort_by_key(|(_, t)| -t); // newest first
    for (path, _) in files.into_iter().skip(keep) {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

/// Spawn a task that snapshots every `interval_ms`, capturing the live
/// cursor so a restart picks up where it left off.
pub fn spawn_snapshot_task(
    registry: SymbolRegistry,
    dir: PathBuf,
    interval_ms: u64,
    cursor: Cursor,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
        // Skip the first immediate tick.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let state = PersistedState::from_registry(&registry, &cursor).await;
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