use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::types::{DeviceId, DeviceState, Command, Device};
use crate::resolver::ConflictRecord;
use crate::reconciler::ReconciliationReport;
use crate::rule_types::{RuleSummary, InFlightSummary};

#[derive(Debug, Clone)]
pub struct UiCommand {
    pub device_id:  String,
    pub attribute:  String,
    pub value:      String,
    pub command_id: String,
}

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

    /// Report from the most recent boot reconciliation.
    /// None until the first reconciliation completes.
    /// UI returns {"status":"not_yet_reconciled"} when None.
    pub last_reconciliation: Option<ReconciliationReport>,

    /// Rule summaries — updated on load and hot-reload only.
    /// Stable between reloads, no need to update on every tick.
    pub rule_summaries: Vec<RuleSummary>,

    /// In-flight delayed actions — updated on every tick alongside
    /// pending_commands. Volatile — reflects live rule engine state.
    pub in_flight: Vec<InFlightSummary>,

    pub registry: Vec<Device>, 
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