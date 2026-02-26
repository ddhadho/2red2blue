use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;
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
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::Text(s) => write!(f, "{}", s),
            Value::Null => write!(f, "null"),
        }
    }
}

// ── Events ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,                  // ULID
    pub sequence: u64,               // WAL position
    pub timestamp: u64,              // unix millis
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
            sequence: 0,             // assigned by WAL on append
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

// ── Device State ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub device_id: DeviceId,
    pub desired: HashMap<AttributeKey, Value>,
    pub actual: HashMap<AttributeKey, Value>,
    pub confidence: f32,
    pub last_seen: u64,              // unix millis
    pub desired_set_at: u64,
    pub desired_set_by: EventSource,
}

impl DeviceState {
    pub fn new(device_id: DeviceId) -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        Self {
            device_id,
            desired: HashMap::new(),
            actual: HashMap::new(),
            confidence: 0.0,         // unknown until first report
            last_seen: 0,
            desired_set_at: now,
            desired_set_by: EventSource::System,
        }
    }

    pub fn update_actual(&mut self, attribute: AttributeKey, value: Value) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        self.actual.insert(attribute, value);
        self.confidence = 1.0;
        self.last_seen = now;
    }
}

// ── State Update ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StateUpdate {
    pub changed_devices: Vec<DeviceId>,
    pub confidence_degraded: Vec<DeviceId>,
}

// ── Commands ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub id: String,                  // ULID
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

// ── Adapter types ─────────────────────────────────────────────

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