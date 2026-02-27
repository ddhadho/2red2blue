use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};
use crate::types::{Event, EventKind};

// ── Priority ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum EventPriority {
    Critical,  // command lifecycle — bypass buffer, write immediately
    Normal,    // everything else — buffered, group commit
}

impl EventPriority {
    pub fn for_kind(kind: &EventKind) -> Self {
        match kind {
            EventKind::CommandSent
            | EventKind::CommandConfirmed
            | EventKind::CommandFailed
            | EventKind::SystemBoot
            | EventKind::SystemShutdown => EventPriority::Critical,
            _ => EventPriority::Normal,
        }
    }
}

// ── Config ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WalConfig {
    pub db_path: String,
    pub snapshot_path: String,
    pub snapshot_interval_events: u64,
    pub buffer_max_bytes: usize,    // flush when buffer exceeds this
    pub buffer_max_age_secs: u64,   // flush when buffer is this old
}

impl WalConfig {
    pub fn for_linux(wal_path: &str, snapshot_path: &str) -> Self {
        Self {
            db_path: format!("{}/wal.db", wal_path),
            snapshot_path: snapshot_path.to_string(),
            snapshot_interval_events: 1000,
            buffer_max_bytes: 4096,
            buffer_max_age_secs: 5,   // flush every 5s on capable hardware
        }
    }

    pub fn for_openwrt(wal_path: &str, snapshot_path: &str) -> Self {
        Self {
            db_path: format!("{}/wal.db", wal_path),
            snapshot_path: snapshot_path.to_string(),
            snapshot_interval_events: 1000,
            buffer_max_bytes: 4096,
            buffer_max_age_secs: 30,  // flush every 30s on router hardware
        }
    }
}

// ── WAL ──────────────────────────────────────────────────────

pub struct Wal {
    conn: Connection,
    config: WalConfig,

    // Group commit buffer
    buffer: Vec<Event>,
    buffer_bytes: usize,
    buffer_since: SystemTime,

    // Sequence tracking
    next_sequence: u64,

    // Snapshot tracking
    events_since_snapshot: u64,
}

impl Wal {
    pub fn open(config: WalConfig) -> Result<Self, WalError> {
        // Ensure directories exist
        if let Some(parent) = Path::new(&config.db_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| WalError::IoError(e.to_string()))?;
        }
        std::fs::create_dir_all(&config.snapshot_path)
            .map_err(|e| WalError::IoError(e.to_string()))?;

        let conn = Connection::open(&config.db_path)
            .map_err(|e| WalError::DbError(e.to_string()))?;

        // Enable WAL mode — critical for concurrent reads during writes
        conn.execute_batch("
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA cache_size=1000;
            PRAGMA temp_store=memory;
        ").map_err(|e| WalError::DbError(e.to_string()))?;

        // Create schema
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS events (
                sequence    INTEGER PRIMARY KEY,
                id          TEXT NOT NULL,
                timestamp   INTEGER NOT NULL,
                source      TEXT NOT NULL,
                kind        TEXT NOT NULL,
                payload     TEXT NOT NULL,
                priority    TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS snapshots (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                sequence    INTEGER NOT NULL,
                created_at  INTEGER NOT NULL,
                path        TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_events_sequence 
                ON events(sequence);
        ").map_err(|e| WalError::DbError(e.to_string()))?;

        // Find next sequence number
        let next_sequence: u64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM events",
                [],
                |row| row.get(0),
            )
            .map_err(|e| WalError::DbError(e.to_string()))?;

        info!(
            db_path = %config.db_path,
            next_sequence = next_sequence,
            "WAL opened"
        );

        Ok(Self {
            conn,
            config,
            buffer: Vec::new(),
            buffer_bytes: 0,
            buffer_since: SystemTime::now(),
            next_sequence,
            events_since_snapshot: 0,
        })
    }

    // ── Append ───────────────────────────────────────────────

    pub fn append(
        &mut self,
        mut event: Event,
        priority: EventPriority,
    ) -> Result<u64, WalError> {
        event.sequence = self.next_sequence;
        self.next_sequence += 1;

        let seq = event.sequence;

        match priority {
            EventPriority::Critical => {
                // Flush any pending buffer first
                if !self.buffer.is_empty() {
                    self.flush_buffer()?;
                }
                // Write immediately with full sync
                self.write_event_sync(&event)?;
                debug!(sequence = seq, "critical event written (sync)");
            }
            EventPriority::Normal => {
                // Estimate serialized size
                let approx_size = event.id.len()
                    + event.payload.len() * 20
                    + 64;

                self.buffer.push(event);
                self.buffer_bytes += approx_size;

                // Check flush conditions
                if self.should_flush() {
                    self.flush_buffer()?;
                }

                debug!(
                    sequence = seq,
                    buffer_size = self.buffer.len(),
                    "normal event buffered"
                );
            }
        }

        self.events_since_snapshot += 1;

        // Snapshot if threshold reached
        if self.events_since_snapshot >= self.config.snapshot_interval_events {
            self.maybe_snapshot()?;
        }

        Ok(seq)
    }

    // ── Flush ────────────────────────────────────────────────

    pub fn flush(&mut self) -> Result<(), WalError> {
        if !self.buffer.is_empty() {
            self.flush_buffer()?;
        }
        Ok(())
    }

    fn should_flush(&self) -> bool {
        if self.buffer_bytes >= self.config.buffer_max_bytes {
            return true;
        }

        if let Ok(elapsed) = self.buffer_since.elapsed() {
            if elapsed >= Duration::from_secs(self.config.buffer_max_age_secs) {
                return true;
            }
        }

        false
    }

    fn flush_buffer(&mut self) -> Result<(), WalError> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        let count = self.buffer.len();

        // Batch insert in single transaction
        let tx = self.conn.unchecked_transaction()
            .map_err(|e| WalError::DbError(e.to_string()))?;

        {
            let mut stmt = tx.prepare_cached("
                INSERT INTO events 
                    (sequence, id, timestamp, source, kind, payload, priority)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ").map_err(|e| WalError::DbError(e.to_string()))?;

            for event in &self.buffer {
                stmt.execute(params![
                    event.sequence,
                    event.id,
                    event.timestamp as i64,
                    serde_json::to_string(&event.source)
                        .map_err(|e| WalError::SerializeError(e.to_string()))?,
                    serde_json::to_string(&event.kind)
                        .map_err(|e| WalError::SerializeError(e.to_string()))?,
                    serde_json::to_string(&event.payload)
                        .map_err(|e| WalError::SerializeError(e.to_string()))?,
                    "normal",
                ]).map_err(|e| WalError::DbError(e.to_string()))?;
            }
        }

        tx.commit().map_err(|e| WalError::DbError(e.to_string()))?;

        debug!(count = count, "buffer flushed to SQLite");

        self.buffer.clear();
        self.buffer_bytes = 0;
        self.buffer_since = SystemTime::now();

        Ok(())
    }

    fn write_event_sync(&mut self, event: &Event) -> Result<(), WalError> {
        self.conn.execute(
            "INSERT INTO events 
                (sequence, id, timestamp, source, kind, payload, priority)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.sequence,
                event.id,
                event.timestamp as i64,
                serde_json::to_string(&event.source)
                    .map_err(|e| WalError::SerializeError(e.to_string()))?,
                serde_json::to_string(&event.kind)
                    .map_err(|e| WalError::SerializeError(e.to_string()))?,
                serde_json::to_string(&event.payload)
                    .map_err(|e| WalError::SerializeError(e.to_string()))?,
                "critical",
            ],
        ).map_err(|e| WalError::DbError(e.to_string()))?;

        // Force sync for critical events
        self.conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
            .map_err(|e| WalError::DbError(e.to_string()))?;

        Ok(())
    }

    // ── Replay ───────────────────────────────────────────────

    pub fn replay_from(&self, sequence: u64) -> Result<Vec<Event>, WalError> {
        let mut stmt = self.conn.prepare(
            "SELECT sequence, id, timestamp, source, kind, payload 
             FROM events 
             WHERE sequence >= ?1 
             ORDER BY sequence ASC"
        ).map_err(|e| WalError::DbError(e.to_string()))?;

        let events = stmt.query_map(params![sequence as i64], |row| {
            Ok(RawRow {
                sequence: row.get::<_, i64>(0)? as u64,
                id: row.get(1)?,
                timestamp: row.get::<_, i64>(2)? as u64,
                source: row.get(3)?,
                kind: row.get(4)?,
                payload: row.get(5)?,
            })
        })
        .map_err(|e| WalError::DbError(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| WalError::DbError(e.to_string()))?;

        events.into_iter()
            .map(|row| row.into_event())
            .collect()
    }

    pub fn latest_sequence(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    pub fn len(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }

    // ── Snapshot ─────────────────────────────────────────────

    fn maybe_snapshot(&mut self) -> Result<(), WalError> {
        let sequence = self.latest_sequence();
        let path = format!(
            "{}/snap_{:010}.json",
            self.config.snapshot_path,
            sequence
        );

        // Record snapshot in DB
        self.conn.execute(
            "INSERT INTO snapshots (sequence, created_at, path) 
             VALUES (?1, ?2, ?3)",
            params![
                sequence as i64,
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
                &path,
            ],
        ).map_err(|e| WalError::DbError(e.to_string()))?;

        self.events_since_snapshot = 0;

        info!(sequence = sequence, path = %path, "snapshot recorded");

        // Clean old snapshots — keep last 2
        self.conn.execute(
            "DELETE FROM snapshots WHERE id NOT IN (
                SELECT id FROM snapshots 
                ORDER BY sequence DESC 
                LIMIT 2
            )",
            [],
        ).map_err(|e| WalError::DbError(e.to_string()))?;

        Ok(())
    }

    pub fn latest_snapshot_sequence(&self) -> Result<Option<u64>, WalError> {
        let result = self.conn.query_row(
            "SELECT sequence FROM snapshots ORDER BY sequence DESC LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        );

        match result {
            Ok(seq) => Ok(Some(seq as u64)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(WalError::DbError(e.to_string())),
        }
    }
}

// ── Raw row helper ───────────────────────────────────────────

struct RawRow {
    sequence: u64,
    id: String,
    timestamp: u64,
    source: String,
    kind: String,
    payload: String,
}

impl RawRow {
    fn into_event(self) -> Result<Event, WalError> {
        Ok(Event {
            sequence: self.sequence,
            id: self.id,
            timestamp: self.timestamp,
            source: serde_json::from_str(&self.source)
                .map_err(|e| WalError::SerializeError(e.to_string()))?,
            kind: serde_json::from_str(&self.kind)
                .map_err(|e| WalError::SerializeError(e.to_string()))?,
            payload: serde_json::from_str(&self.payload)
                .map_err(|e| WalError::SerializeError(e.to_string()))?,
        })
    }
}

// ── Errors ───────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum WalError {
    #[error("Database error: {0}")]
    DbError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Serialization error: {0}")]
    SerializeError(String),
}