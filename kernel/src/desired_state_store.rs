use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tracing::{info, warn};
use crate::types::{AttributeKey, DeviceId, Value};

// ── Snapshot format ──────────────────────────────────────────

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Snapshot {
    version: u32,
    written_at: u64,
    // device_id → attribute → value
    devices: HashMap<String, HashMap<String, Value>>,
}

// ── Store ────────────────────────────────────────────────────

pub struct DesiredStateStore {
    path: String,
    state: HashMap<DeviceId, HashMap<AttributeKey, Value>>,
    dirty: bool,
    last_flush: SystemTime,
    flush_interval: Duration,
}

impl DesiredStateStore {
    // Load from snapshot file. If the file does not exist (first boot),
    // returns an empty store — no error, no desired state.
    pub fn load(path: &str) -> Result<Self, StoreError> {
        let state = if std::path::Path::new(path).exists() {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| StoreError::Io(e.to_string()))?;

            let snapshot: Snapshot = serde_json::from_str(&raw)
                .map_err(|e| StoreError::Deserialize(e.to_string()))?;

            if snapshot.version != 1 {
                warn!(
                    version = snapshot.version,
                    path = %path,
                    "unknown snapshot version — ignoring, starting empty"
                );
                HashMap::new()
            } else {
                let mut state: HashMap<DeviceId, HashMap<AttributeKey, Value>> =
                    HashMap::new();

                for (device_id, attrs) in snapshot.devices {
                    let entry = state
                        .entry(DeviceId(device_id))
                        .or_default();

                    for (attr, value) in attrs {
                        entry.insert(AttributeKey(attr), value);
                    }
                }

                info!(
                    path = %path,
                    devices = state.len(),
                    "desired state snapshot loaded"
                );

                state
            }
        } else {
            info!(path = %path, "no desired state snapshot found — starting empty");
            HashMap::new()
        };

        Ok(Self {
            path: path.to_string(),
            state,
            dirty: false,
            last_flush: SystemTime::now(),
            flush_interval: Duration::from_secs(30),
        })
    }

    // ── Mutation ─────────────────────────────────────────────

    // Called by main whenever set_desired is called on the state engine.
    // Marks dirty — does not write immediately.
    pub fn set(
        &mut self,
        device_id: &DeviceId,
        attr: AttributeKey,
        value: Value,
    ) {
        self.state
            .entry(device_id.clone())
            .or_default()
            .insert(attr, value);

        self.dirty = true;
    }

    // ── Read ─────────────────────────────────────────────────

    pub fn get_all(&self) -> &HashMap<DeviceId, HashMap<AttributeKey, Value>> {
        &self.state
    }

    // ── Flush ────────────────────────────────────────────────

    // Called in tick branch — flushes if dirty and interval has elapsed.
    pub fn flush_if_needed(&mut self, now: SystemTime) -> Result<(), StoreError> {
        if !self.dirty {
            return Ok(());
        }

        let elapsed = now
            .duration_since(self.last_flush)
            .unwrap_or_default();

        if elapsed >= self.flush_interval {
            self.flush()?;
        }

        Ok(())
    }

    // Forced flush — called on clean shutdown before wal.flush().
    pub fn flush(&mut self) -> Result<(), StoreError> {
        if !self.dirty {
            return Ok(());
        }

        let mut devices: HashMap<String, HashMap<String, Value>> = HashMap::new();

        for (device_id, attrs) in &self.state {
            let entry = devices.entry(device_id.0.clone()).or_default();
            for (attr, value) in attrs {
                entry.insert(attr.0.clone(), value.clone());
            }
        }

        let snapshot = Snapshot {
            version: 1,
            written_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            devices,
        };

        let json = serde_json::to_string_pretty(&snapshot)
            .map_err(|e| StoreError::Serialize(e.to_string()))?;

        // Write to temp file then rename — atomic on Linux, avoids partial writes
        let tmp_path = format!("{}.tmp", self.path);
        std::fs::write(&tmp_path, &json)
            .map_err(|e| StoreError::Io(e.to_string()))?;
        std::fs::rename(&tmp_path, &self.path)
            .map_err(|e| StoreError::Io(e.to_string()))?;

        self.dirty = false;
        self.last_flush = SystemTime::now();

        info!(
            path = %self.path,
            devices = self.state.len(),
            "desired state snapshot flushed"
        );

        Ok(())
    }
}

// ── Errors ───────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("IO error: {0}")]
    Io(String),

    #[error("Serialize error: {0}")]
    Serialize(String),

    #[error("Deserialize error: {0}")]
    Deserialize(String),
}