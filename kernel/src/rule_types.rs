use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::types::{DeviceId, AttributeKey, Value, RuleId};

// ── TOML shape ───────────────────────────────────────────────
// Raw deserialized structs — mirrors rules.toml exactly.
// Strings everywhere, no validation, no conversion.

#[derive(Debug, Serialize, Deserialize)]
pub struct RuleFile {
    pub rules: Vec<RuleEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RuleEntry {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: u8,
    pub conflict_group: String,
    pub trigger: TriggerEntry,
    #[serde(default)]
    pub conditions: Vec<ConditionEntry>,
    pub actions: Vec<ActionEntry>,
    pub stateful: Option<StatefulEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TriggerEntry {
    pub kind: String,
    pub device_id: Option<String>,
    pub attribute: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConditionEntry {
    pub subject_device_id: String,
    pub subject_attribute: String,
    pub operator: String,
    // Typed value fields — only one should be set per condition.
    // IsUnknown sets none. Between sets value_range_min + value_range_max.
    pub value_text: Option<String>,
    pub value_float: Option<f64>,
    pub value_int: Option<i64>,
    pub value_bool: Option<bool>,
    pub value_range_min: Option<f64>,
    pub value_range_max: Option<f64>,
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActionEntry {
    pub device_id: String,
    pub attribute: String,
    pub value_text: Option<String>,
    pub value_float: Option<f64>,
    pub value_int: Option<i64>,
    pub value_bool: Option<bool>,
    pub delay_seconds: Option<u64>,
    #[serde(default)]
    pub params: HashMap<String, toml::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StatefulEntry {
    pub timeout_seconds: u64,
    #[serde(default)]
    pub on_timeout_actions: Vec<ActionEntry>,
}

// ── Parsed types ─────────────────────────────────────────────
// Clean internal types after validation and conversion.
// DeviceId not String, Operator enum not "Equals", etc.
// These are what the rule engine works with.

#[derive(Debug, Clone)]
pub struct Rule {
    pub id: RuleId,
    pub name: String,
    pub enabled: bool,
    pub priority: u8,
    pub conflict_group: String,
    pub trigger: Trigger,
    pub conditions: Vec<Condition>,
    pub actions: Vec<Action>,
    pub stateful: Option<StatefulConfig>,
}

#[derive(Debug, Clone)]
pub enum Trigger {
    DeviceStateChanged {
        device_id: DeviceId,
        attribute: AttributeKey,
    },
}

#[derive(Debug, Clone)]
pub struct Condition {
    pub device_id: DeviceId,
    pub attribute: AttributeKey,
    pub operator: Operator,
    pub value: ConditionValue,
    pub duration_seconds: Option<u64>,
}

/// The value(s) a condition compares against.
/// IsUnknown → None (no comparison value needed)
/// Between   → Range (two bounds)
/// all others → Single
#[derive(Debug, Clone)]
pub enum ConditionValue {
    None,
    Single(Value),
    Range(Value, Value),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    Between,
    Changed,
    WasPreviously,
    IsUnknown,
}

#[derive(Debug, Clone)]
pub struct Action {
    pub device_id: DeviceId,
    pub attribute: AttributeKey,
    pub value: Value,
    pub params: HashMap<String, Value>, 
    pub delay_seconds: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct StatefulConfig {
    pub timeout_seconds: u64,
    pub on_timeout_actions: Vec<Action>,
}

// ── Runtime state ────────────────────────────────────────────
// Created at runtime, mutated as rules fire and timers tick.
// Serializable so in-flight state can be written to the WAL.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleState {
    pub rule_id: String,
    /// When each duration condition first became true.
    /// Key is condition index within the rule's conditions vec.
    pub condition_met_at: HashMap<usize, u64>,  // unix millis
    /// All active in-flight entries for this rule.
    pub in_flight: Vec<InFlightEntry>,
}

impl RuleState {
    pub fn new(rule_id: &str) -> Self {
        Self {
            rule_id: rule_id.to_string(),
            condition_met_at: HashMap::new(),
            in_flight: Vec::new(),
        }
    }

    pub fn has_timeout(&self) -> bool {
        self.in_flight.iter().any(|e| e.kind == InFlightKind::Timeout)
    }

    pub fn clear_timeout(&mut self) {
        self.in_flight.retain(|e| e.kind != InFlightKind::Timeout);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InFlightEntry {
    /// Unix millis when this entry should fire
    pub execute_at: u64,
    /// Actions to dispatch when execute_at is reached
    pub actions: Vec<SerializedAction>,
    pub kind: InFlightKind,
}

/// Action serialized for WAL storage — plain strings, no DeviceId/AttributeKey wrappers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedAction {
    pub device_id: String,
    pub attribute: String,
    pub value: Value,
    #[serde(default)]
    pub params: HashMap<String, Value>,  
}

impl SerializedAction {
    pub fn from_action(action: &Action) -> Self {
        Self {
            device_id: action.device_id.0.clone(),
            attribute: action.attribute.0.clone(),
            value: action.value.clone(),
            params: action.params.clone(),
        }
    }

    pub fn to_action(&self) -> Action {
        Action {
            device_id: DeviceId(self.device_id.clone()),
            attribute: AttributeKey(self.attribute.clone()),
            value: self.value.clone(),
            params: self.params.clone(), 
            delay_seconds: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InFlightKind {
    DelayedAction,  // from action.delay_seconds
    Timeout,        // from StatefulConfig.timeout_seconds
}

// ── UI summary types ─────────────────────────────────────────
// Shallow read-only projections for the dashboard.
// No condition trees, no action details — just what the UI needs.
// Written to SharedState, served at GET /rules.

/// One row in the rules panel — identity, status, and scheduling metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSummary {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: u8,
    pub conflict_group: String,
}

impl RuleSummary {
    pub fn from_rule(rule: &Rule) -> Self {
        Self {
            id: rule.id.0.clone(),
            name: rule.name.clone(),
            enabled: rule.enabled,
            priority: rule.priority,
            conflict_group: rule.conflict_group.clone(),
        }
    }
}

/// One row in the in-flight panel — a delayed action waiting to fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InFlightSummary {
    pub rule_id: String,
    /// First action's device and attribute — representative of what will fire.
    /// Most in-flight entries have one action; if multiple, first is shown.
    pub device_id: String,
    pub attribute: String,
    pub value: Value,
    /// Unix millis when this entry fires.
    pub fires_at_ms: u64,
    pub kind: InFlightKind,
}

// ── Hot reload report ────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HotReloadReport {
    pub rules_loaded: u32,
    pub rules_rejected: u32,
    pub rules_added: u32,
    pub rules_removed: u32,
    pub in_flight_cancelled: u32,
}

// ── Errors ───────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum RuleError {
    #[error("IO error: {0}")]
    IoError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Rule '{0}': unknown operator '{1}'")]
    UnknownOperator(String, String),

    #[error("Rule '{0}': unknown trigger kind '{1}'")]
    UnknownTrigger(String, String),

    #[error("Rule '{0}': references unknown device '{1}'")]
    UnknownDevice(String, String),

    #[error("Rule '{0}': device '{1}' has no attribute '{2}'")]
    UnknownAttribute(String, String, String),

    #[error("Rule '{0}': condition value missing for operator '{1}'")]
    MissingValue(String, String),

    #[error("Rule '{0}': Between operator requires value_range_min and value_range_max")]
    MissingRange(String),
}