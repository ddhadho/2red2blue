use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use ulid::Ulid;

// ── Identifiers ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AttributeKey(pub String);

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for AttributeKey {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Values ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Null,
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Value::Bool(b)   => write!(f, "{}", b),
            Value::Int(i)    => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::Text(s)   => write!(f, "{}", s),
            Value::Null      => write!(f, "null"),
        }
    }
}

// ── Events ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,           // ULID
    pub sequence: u64,        // WAL position
    pub timestamp: u64,       // unix millis
    pub source: EventSource,
    pub kind: EventKind,
    pub payload: HashMap<String, Value>,
}

impl Event {
    pub fn new(
        source: EventSource,
        kind: EventKind,
        payload: HashMap<String, Value>,
    ) -> Self {
        Self {
            id: Ulid::new().to_string(),
            sequence: 0,      // assigned by WAL on append
            timestamp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            source,
            kind,
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventSource {
    Device(DeviceId),
    System,
    Rule(RuleId),
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    DeviceStateChanged,
    CommandSent,
    CommandConfirmed,
    CommandFailed,
    RuleTriggered,
    RuleConflict,
    SystemBoot,
    SystemShutdown,
    ReconciliationStarted,
    ReconciliationCompleted,
}

// ── Device ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    pub external_id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub capabilities: Vec<Capability>,
    pub confidence_decay_seconds: u64,
    pub safe_default: HashMap<AttributeKey, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceKind {
    Gate,
    SecurityLight,
    BoreholePump,
    WaterTank,
    AlarmPanel,
    Camera,
    SmartPlug,
    Inverter,
    Generator,
    PowerMonitor,
    Sensor,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Capability {
    Readable(AttributeKey),
    Writable(AttributeKey),
}

// ── Confidence ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Confidence {
    /// 0.0 = unknown, 1.0 = certain
    pub value: f32,

    /// Grace period — confidence stays at 1.0 for this duration after last_seen.
    /// After this, confidence decays linearly to 0.0 over the same duration again.
    pub decays_after: Duration,

    /// What to act on when confidence is at or below unknown_threshold.
    /// Never written into actual — applied at read time via get_effective.
    pub safe_default: HashMap<AttributeKey, Value>,
}

impl Confidence {
    pub fn new(decays_after: Duration, safe_default: HashMap<AttributeKey, Value>) -> Self {
        Self {
            value: 0.0,   // unknown until first report
            decays_after,
            safe_default,
        }
    }
}

// ── Device State ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub device_id: DeviceId,

    /// What we want the device to be.
    pub desired: HashMap<AttributeKey, Value>,

    /// What the device last told us it is.
    /// Never overwritten by assumptions or safe defaults.
    pub actual: HashMap<AttributeKey, Value>,

    /// The value each attribute held immediately before the most recent
    /// update_actual call. One value deep — not a history.
    /// Used by the WasPreviously operator in the rule engine.
    /// Never touched by confidence decay or safe defaults.
    pub previous: HashMap<AttributeKey, Value>,

    pub confidence: Confidence,

    /// When we last heard from this device.
    /// UNIX_EPOCH means never seen.
    pub last_seen: SystemTime,

    pub desired_set_at: SystemTime,
    pub desired_set_by: EventSource,
}

impl DeviceState {
    pub fn new(device_id: DeviceId, confidence: Confidence) -> Self {
        Self {
            device_id,
            desired: HashMap::new(),
            actual: HashMap::new(),
            previous: HashMap::new(),
            confidence,
            last_seen: SystemTime::UNIX_EPOCH,
            desired_set_at: SystemTime::now(),
            desired_set_by: EventSource::System,
        }
    }

    /// Update actual state from a genuine device report.
    /// Moves current actual value to previous before overwriting.
    /// Resets confidence to 1.0 and updates last_seen.
    pub fn update_actual(&mut self, attribute: AttributeKey, value: Value, now: SystemTime) {
        // Move current actual to previous before overwriting
        if let Some(current) = self.actual.get(&attribute) {
            self.previous.insert(attribute.clone(), current.clone());
        }
        self.actual.insert(attribute, value);
        self.confidence.value = 1.0;
        self.last_seen = now;
    }

    /// The value to act on for this attribute right now.
    ///
    /// - Above unknown_threshold        → value from actual
    /// - At or below, safe default set  → safe default value
    /// - At or below, no safe default   → None
    ///
    /// Rule engine, reconciler, and diff all call this.
    /// Nothing reads actual directly for decisions.
    pub fn get_effective(&self, attr: &AttributeKey, unknown_threshold: f32) -> Option<&Value> {
        if self.confidence.value <= unknown_threshold {
            self.confidence.safe_default.get(attr)
        } else {
            self.actual.get(attr)
        }
    }
}

// ── State Update ─────────────────────────────────────────────

/// Returned by apply_event. Signals the rule engine which devices changed.
/// Confidence degradation is signalled separately via tick.
#[derive(Debug, Clone)]
pub struct StateUpdate {
    pub changed_devices: Vec<DeviceId>,
}

// ── State Mismatch ────────────────────────────────────────────

/// A mismatch between desired and actual for a single attribute.
/// Returned by StateEngine::diff. Carries confidence so the reconciler
/// can decide whether to act immediately or poll first.
#[derive(Debug, Clone)]
pub struct StateMismatch {
    pub device_id: DeviceId,
    pub attribute: AttributeKey,
    pub desired: Value,
    pub actual: Option<Value>,   // None if device has never reported this attribute
    pub confidence: f32,
}

// ── Commands ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: String,              // ULID
    pub rule_id: Option<RuleId>,
    pub device_id: DeviceId,
    pub attribute: AttributeKey,
    pub value: Value,
    pub issued_at: u64,
    pub status: CommandStatus,
    pub retry_count: u8,
}

impl Command {
    pub fn new(
        device_id: DeviceId,
        attribute: AttributeKey,
        value: Value,
        rule_id: Option<RuleId>,
    ) -> Self {
        Self {
            id: Ulid::new().to_string(),
            rule_id,
            device_id,
            attribute,
            value,
            issued_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            status: CommandStatus::Pending,
            retry_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandStatus {
    Pending,
    Sent,
    Confirmed,
    Failed(String),
    Timeout,
}

// ── Adapter Types ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RawDeviceEvent {
    pub external_id: String,
    pub attribute: String,
    pub value: Value,
    pub timestamp: u64,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AdapterCommand {
    pub external_id: String,
    pub attribute: String,
    pub value: Value,
    pub command_id: String,
}