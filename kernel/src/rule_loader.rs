use tracing::{info, warn};
use crate::types::{DeviceId, AttributeKey, Value, RuleId};
use crate::registry::DeviceRegistry;
use crate::rule_types::*;

// ── Public entry point ───────────────────────────────────────

/// Load and validate rules from a TOML file against the device registry.
///
/// Invalid rules are logged as warnings and skipped — the daemon never
/// crashes on a bad rule file. Returns Err only if the file cannot be
/// read or parsed at all.

pub fn load_rules(
    path: &str,
    registry: &DeviceRegistry,
) -> Result<Vec<Rule>, RuleError> {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            info!(path = %path, "rules file not found — starting with no rules");
            return Ok(vec![]);
        }
        Err(e) => return Err(RuleError::IoError(e.to_string())),
    };

    if contents.trim().is_empty() {
        info!(path = %path, "rules file empty — starting with no rules");
        return Ok(vec![]);
    }

    let file: RuleFile = toml::from_str(&contents)
        .map_err(|e| RuleError::ParseError(e.to_string()))?;

    let mut rules = vec![];

    for entry in file.rules {
        if !entry.enabled {
            continue;
        }

        match parse_rule(entry, registry) {
            Ok(rule) => rules.push(rule),
            Err(e) => warn!(error = %e, "rule rejected — skipping"),
        }
    }

    info!(count = rules.len(), path = %path, "rules loaded");

    Ok(rules)
}

// ── Rule parsing ─────────────────────────────────────────────

fn parse_rule(entry: RuleEntry, registry: &DeviceRegistry) -> Result<Rule, RuleError> {
    let trigger   = parse_trigger(&entry, registry)?;
    let conditions = parse_conditions(&entry, registry)?;
    let actions   = parse_actions(&entry.actions, &entry.id, registry)?;
    let stateful  = parse_stateful(&entry, registry)?;

    Ok(Rule {
        id: RuleId(entry.id),
        name: entry.name,
        enabled: entry.enabled,
        priority: entry.priority,
        conflict_group: entry.conflict_group,
        trigger,
        conditions,
        actions,
        stateful,
    })
}

fn parse_trigger(entry: &RuleEntry, registry: &DeviceRegistry) -> Result<Trigger, RuleError> {
    match entry.trigger.kind.as_str() {
        "DeviceStateChanged" => {
            let device_id_str = entry.trigger.device_id
                .as_ref()
                .ok_or_else(|| RuleError::UnknownTrigger(
                    "DeviceStateChanged requires device_id".to_string(),
                    entry.id.clone(),
                ))?;

            let attribute_str = entry.trigger.attribute
                .as_ref()
                .ok_or_else(|| RuleError::UnknownTrigger(
                    "DeviceStateChanged requires attribute".to_string(),
                    entry.id.clone(),
                ))?;

            let device_id = DeviceId(device_id_str.clone());
            if registry.get(&device_id).is_none() {
                return Err(RuleError::UnknownDevice(
                    entry.id.clone(),
                    device_id_str.clone(),
                ));
            }

            Ok(Trigger::DeviceStateChanged {
                device_id,
                attribute: AttributeKey(attribute_str.clone()),
            })
        }
        other => Err(RuleError::UnknownTrigger(
            other.to_string(),
            entry.id.clone(),
        )),
    }
}

fn parse_conditions(entry: &RuleEntry, registry: &DeviceRegistry) -> Result<Vec<Condition>, RuleError> {
    entry.conditions.iter().map(|c| {
        let operator = parse_operator(&c.operator, &entry.id)?;

        let device_id = DeviceId(c.subject_device_id.clone());
        if registry.get(&device_id).is_none() {
            return Err(RuleError::UnknownDevice(
                entry.id.clone(),
                c.subject_device_id.clone(),
            ));
        }

        let value = parse_condition_value(c, &operator, &entry.id)?;

        Ok(Condition {
            device_id,
            attribute: AttributeKey(c.subject_attribute.clone()),
            operator,
            value,
            duration_seconds: c.duration_seconds,
        })
    }).collect()
}

fn parse_actions(
    entries: &[ActionEntry],
    rule_id: &str,
    registry: &DeviceRegistry,
) -> Result<Vec<Action>, RuleError> {
    entries.iter().map(|a| {
        let device_id = DeviceId(a.device_id.clone());
        if registry.get(&device_id).is_none() {
            return Err(RuleError::UnknownDevice(
                rule_id.to_string(),
                a.device_id.clone(),
            ));
        }

        let params = a.params.iter()
            .map(|(k, v)| (k.clone(), toml_to_value(v)))
            .collect();

        Ok(Action {
            device_id,
            attribute: AttributeKey(a.attribute.clone()),
            value: parse_action_value(a),
            params,                      // NEW
            delay_seconds: a.delay_seconds,
        })
    }).collect()
}

fn parse_stateful(entry: &RuleEntry, registry: &DeviceRegistry) -> Result<Option<StatefulConfig>, RuleError> {
    match &entry.stateful {
        None => Ok(None),
        Some(s) => {
            let on_timeout_actions = parse_actions(
                &s.on_timeout_actions,
                &entry.id,
                registry,
            )?;
            Ok(Some(StatefulConfig {
                timeout_seconds: s.timeout_seconds,
                on_timeout_actions,
            }))
        }
    }
}

// ── Operator parsing ─────────────────────────────────────────

fn parse_operator(op: &str, rule_id: &str) -> Result<Operator, RuleError> {
    match op {
        "Equals"        => Ok(Operator::Equals),
        "NotEquals"     => Ok(Operator::NotEquals),
        "GreaterThan"   => Ok(Operator::GreaterThan),
        "LessThan"      => Ok(Operator::LessThan),
        "Between"       => Ok(Operator::Between),
        "Changed"       => Ok(Operator::Changed),
        "WasPreviously" => Ok(Operator::WasPreviously),
        "IsUnknown"     => Ok(Operator::IsUnknown),
        other => Err(RuleError::UnknownOperator(
            other.to_string(),
            rule_id.to_string(),
        )),
    }
}

// ── Value parsing ────────────────────────────────────────────

fn parse_condition_value(
    c: &ConditionEntry,
    operator: &Operator,
    rule_id: &str,
) -> Result<ConditionValue, RuleError> {
    match operator {
        // These check confidence or StateUpdate — no comparison value needed
        Operator::IsUnknown | Operator::Changed => Ok(ConditionValue::None),

        // Between needs two bounds
        Operator::Between => {
            let min = c.value_range_min.ok_or_else(||
                RuleError::MissingRange(rule_id.to_string()))?;
            let max = c.value_range_max.ok_or_else(||
                RuleError::MissingRange(rule_id.to_string()))?;
            Ok(ConditionValue::Range(Value::Float(min), Value::Float(max)))
        }

        // All others need a single value
        _ => {
            let val = parse_single_value(c).ok_or_else(|| {
                RuleError::MissingValue(
                    rule_id.to_string(),
                    format!("{:?}", operator),
                )
            })?;
            Ok(ConditionValue::Single(val))
        }
    }
}

fn parse_single_value(c: &ConditionEntry) -> Option<Value> {
    if let Some(v) = &c.value_text { return Some(Value::Text(v.clone())); }
    if let Some(v) = c.value_float { return Some(Value::Float(v)); }
    if let Some(v) = c.value_int   { return Some(Value::Int(v)); }
    if let Some(v) = c.value_bool  { return Some(Value::Bool(v)); }
    None
}

fn parse_action_value(a: &ActionEntry) -> Value {
    if let Some(v) = &a.value_text { return Value::Text(v.clone()); }
    if let Some(v) = a.value_float { return Value::Float(v); }
    if let Some(v) = a.value_int   { return Value::Int(v); }
    if let Some(v) = a.value_bool  { return Value::Bool(v); }
    Value::Null
}

fn toml_to_value(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s)  => Value::Text(s.clone()),
        toml::Value::Integer(i) => Value::Int(*i),
        toml::Value::Float(f)   => Value::Float(*f),
        toml::Value::Boolean(b) => Value::Bool(*b),
        // Value has no array/table variant — fall back to a debug string
        // rather than silently dropping the param. Extend Value if you
        // need real array/table params later.
        other => Value::Text(format!("{:?}", other)),
    }
}