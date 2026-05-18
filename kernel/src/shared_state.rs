use std::collections::VecDeque;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
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

#[derive(Debug, Clone, Serialize)]
pub struct EventSummary {
    pub timestamp:  u64,
    pub kind:       String,
    pub device_id:  Option<String>,
    pub attribute:  Option<String>,
    pub value:      Option<String>,
    pub source:     String,
    pub command_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SseMessage {
    pub event: String,
    pub data:  String,
}

// ── SharedState ──────────────────────────────────────────────
//
// Single struct behind a single lock. Main updates it after every
// event, tick, and dispatch outcome. UI clones what it needs under
// the lock and releases immediately before serializing.
//
// One lock — no possibility of two separate states drifting apart.

#[derive(Debug, Clone)]
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
    pub event_history:      VecDeque<EventSummary>,
    pub rules_path: String,
    pub ha_url:      String,
    pub ha_token:    String,
    pub devices_path: String,
    pub config_path:  String,
    pub sse_tx: broadcast::Sender<SseMessage>,
}

impl SharedState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub type Shared = Arc<Mutex<SharedState>>;

pub fn new_shared() -> Shared {
    let (sse_tx, _) = broadcast::channel(256);
    Arc::new(Mutex::new(SharedState {
        sse_tx,
        ..Default::default()
    }))
}

impl Default for SharedState {
    fn default() -> Self {
        let (sse_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            devices:             HashMap::new(),
            conflicts:           Vec::new(),
            pending_commands:    Vec::new(),
            last_reconciliation: None,
            rule_summaries:      Vec::new(),
            in_flight:           Vec::new(),
            registry:            Vec::new(),
            event_history:       std::collections::VecDeque::new(),
            rules_path:          String::new(),
            devices_path:        String::new(),
            config_path:         String::new(),
            ha_url:              String::new(),
            ha_token:            String::new(),
            sse_tx,
        }
    }
}

