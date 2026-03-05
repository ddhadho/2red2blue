use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::types::{DeviceId, DeviceState, Command};
use crate::resolver::ConflictRecord;

// ── SharedState ──────────────────────────────────────────────
//
// Single struct behind a single lock. Main updates it after every
// event, tick, and dispatch outcome. UI clones what it needs under
// the lock and releases immediately before serializing.
//
// One lock — no possibility of two separate states drifting apart.

#[derive(Debug, Clone, Default)]
pub struct SharedState {
    /// Current device states — updated after every event and tick
    pub devices: HashMap<DeviceId, DeviceState>,

    /// Conflict records — appended when rules compete, never pruned in V1
    pub conflicts: Vec<ConflictRecord>,

    /// Commands currently tracked by the dispatcher — in-flight or pending
    pub pending_commands: Vec<Command>,
}

impl SharedState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub type Shared = Arc<Mutex<SharedState>>;

pub fn new_shared() -> Shared {
    Arc::new(Mutex::new(SharedState::new()))
}