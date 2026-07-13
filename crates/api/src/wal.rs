//! Write-ahead log (WAL) for the matching engine + ledger state.
//!
//! ## Phase 5 design
//!
//! The WAL records **inputs** (order submissions and cancellations),
//! not outputs (trades, settled balances). Both the matching engine
//! and the ledger are deterministic given the same input sequence, so
//! replaying the WAL produces an identical state to live operation.
//!
//! File format: JSONL — one event per line. The first line is a
//! `WalHeader` (schema version + checkpoint). Subsequent lines are
//! `WalEvent`s. Append-only with `flush()` after each line for
//! durability.
//!
//! ## Replay
//!
//! `Wal::replay_all` reads every event in order. The caller (the
//! bootstrapper in `service.rs`) re-applies each event:
//!
//! - Submit: `ledger.place(…)` (lock funds) →
//!           `actor.submit_limit(…)` (match) →
//!           for each returned trade, `ledger.settle_trade(…)`.
//! - Cancel: `actor.cancel(…)` → `ledger.cancel(…)`.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum WalError {
    #[error("io: {0}")]
    Io(String),
    #[error("json: {0}")]
    Json(String),
    #[error("corrupt line {line_no}: {detail}")]
    Corrupt { line_no: usize, detail: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalHeader {
    pub schema_version: u32,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum WalAction {
    Submit,
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEvent {
    /// Monotonic sequence number. Used by readers to detect gaps.
    pub seq: u64,
    pub action: WalAction,
    pub order_id: u64,
    /// 0 for Cancel (cancel doesn't need user_id — looked up from ledger).
    pub user_id: u64,
    /// Trading symbol, e.g. "BTC-USDC".
    pub symbol: String,
    /// 0 = Buy, 1 = Sell.
    pub side: u8,
    /// 0 = Limit, 1 = Market.
    pub order_kind: u8,
    pub price: Option<u64>,
    pub qty: u64,
    pub timestamp: u64,
}

pub const SCHEMA_VERSION: u32 = 1;

/// Open or create a WAL at `path`. Writes a header if the file is
/// empty, then returns a handle for appending.
pub struct Wal {
    path: PathBuf,
    writer: BufWriter<File>,
    next_seq: u64,
}

impl Wal {
    pub fn open_or_create(path: &Path) -> Result<Self, WalError> {
        let exists = path.exists();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)
            .map_err(|e| WalError::Io(e.to_string()))?;
        let mut writer = BufWriter::new(file);

        let next_seq = if !exists {
            // Fresh file. Write header. `writer` is borrowed mutably
            // by the writeln!/flush! calls; we don't move it.
            let header = WalHeader {
                schema_version: SCHEMA_VERSION,
                created_at_unix: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            };
            let line = serde_json::to_string(&header)
                .map_err(|e| WalError::Json(e.to_string()))?;
            writeln!(&mut writer, "{}", line).map_err(|e| WalError::Io(e.to_string()))?;
            writer.flush().map_err(|e| WalError::Io(e.to_string()))?;
            1
        } else {
            // Count existing event lines.
            Self::count_existing_events(path)?
        };

        Ok(Wal {
            path: path.to_owned(),
            writer,
            next_seq,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Append an event to the WAL. Caller is responsible for assigning
    /// the `seq` field; the writer tracks the next expected seq and
    /// updates it after a successful write.
    pub fn append(&mut self, mut event: WalEvent) -> Result<(), WalError> {
        if event.seq == 0 {
            event.seq = self.next_seq;
        }
        let line = serde_json::to_string(&event)
            .map_err(|e| WalError::Json(e.to_string()))?;
        writeln!(self.writer, "{}", line).map_err(|e| WalError::Io(e.to_string()))?;
        self.writer.flush().map_err(|e| WalError::Io(e.to_string()))?;
        self.next_seq = self.next_seq.saturating_add(1);
        Ok(())
    }

    fn count_existing_events(path: &Path) -> Result<u64, WalError> {
        let file = File::open(path).map_err(|e| WalError::Io(e.to_string()))?;
        let reader = BufReader::new(file);
        let mut count: u64 = 0;
        for (i, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| WalError::Io(e.to_string()))?;
            if i == 0 {
                // Header line. Skip it.
                continue;
            }
            // Validate it parses as a WalEvent.
            let _event: WalEvent = serde_json::from_str(&line).map_err(|e| WalError::Corrupt {
                line_no: i + 1,
                detail: e.to_string(),
            })?;
            count += 1;
        }
        Ok(count + 1)
    }

    /// Read every WAL event in order. The header is filtered out; the
    /// reader does not need to know about it.
    pub fn replay_all(path: &Path) -> Result<Vec<WalEvent>, WalError> {
        let file = File::open(path).map_err(|e| WalError::Io(e.to_string()))?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            let line = line.map_err(|e| WalError::Io(e.to_string()))?;
            if i == 0 {
                continue; // header
            }
            let event: WalEvent = serde_json::from_str(&line).map_err(|e| WalError::Corrupt {
                line_no: i + 1,
                detail: e.to_string(),
            })?;
            events.push(event);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fresh_wal_writes_header_and_increments_seq() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.jsonl");
        let mut wal = Wal::open_or_create(&path).unwrap();
        assert_eq!(wal.next_seq(), 1);

        wal.append(WalEvent {
            seq: 0,
            action: WalAction::Submit,
            order_id: 1,
            user_id: 42,
            symbol: "BTC-USDC".into(),
            side: 0,
            order_kind: 0,
            price: Some(100),
            qty: 5,
            timestamp: 1,
        })
        .unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = raw.lines().collect();
        assert_eq!(lines.len(), 2, "header + 1 event");
        // First line is the header.
        let hdr: WalHeader = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(hdr.schema_version, SCHEMA_VERSION);
        // Second line is the event.
        let ev: WalEvent = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(ev.action, WalAction::Submit);
        assert_eq!(ev.order_id, 1);
        // seq was assigned by append() since 0 was passed.
        assert_eq!(ev.seq, 1);
        assert_eq!(wal.next_seq(), 2);
    }

    #[test]
    fn replay_returns_events_in_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.jsonl");
        {
            let mut wal = Wal::open_or_create(&path).unwrap();
            for i in 0..3 {
                wal.append(WalEvent {
                    seq: 0,
                    action: WalAction::Submit,
                    order_id: i + 1,
                    user_id: 100,
                    symbol: "BTC-USDC".into(),
                    side: 0,
                    order_kind: 0,
                    price: Some(100),
                    qty: 1,
                    timestamp: i,
                })
                .unwrap();
            }
        }
        let events = Wal::replay_all(&path).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].order_id, 1);
        assert_eq!(events[1].order_id, 2);
        assert_eq!(events[2].order_id, 3);
        // seqs are 1, 2, 3 (1-based, header is line 0).
        assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn reopen_continues_seq() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.jsonl");
        {
            let mut wal = Wal::open_or_create(&path).unwrap();
            wal.append(WalEvent {
                seq: 0,
                action: WalAction::Cancel,
                order_id: 99,
                user_id: 0,
                symbol: "BTC-USDC".into(),
                side: 0,
                order_kind: 0,
                price: None,
                qty: 0,
                timestamp: 0,
            })
            .unwrap();
            assert_eq!(wal.next_seq(), 2);
        }
        let mut wal2 = Wal::open_or_create(&path).unwrap();
        // Header is line 0; one event; next_seq should be 2.
        assert_eq!(wal2.next_seq(), 2);
        wal2
            .append(WalEvent {
                seq: 0,
                action: WalAction::Cancel,
                order_id: 100,
                user_id: 0,
                symbol: "BTC-USDC".into(),
                side: 0,
                order_kind: 0,
                price: None,
                qty: 0,
                timestamp: 0,
            })
            .unwrap();
        assert_eq!(wal2.next_seq(), 3);
        let events = Wal::replay_all(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
    }

    #[test]
    fn corrupt_line_reports_specific_line_no() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("wal.jsonl");
        std::fs::write(
            &path,
            "{\"schema_version\":1,\"created_at_unix\":0}\n\
             not-a-json-line\n",
        )
        .unwrap();
        let result = Wal::replay_all(&path);
        match result {
            Err(WalError::Corrupt { line_no, .. }) => assert_eq!(line_no, 2),
            other => panic!("expected Corrupt at line 2, got {:?}", other),
        }
    }
}