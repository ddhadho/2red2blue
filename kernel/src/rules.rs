use std::collections::HashMap;
use std::time::SystemTime;
use tracing::{debug, info, warn};
use crate::types::*;
use crate::rule_types::*;

const UNKNOWN_THRESHOLD: f32 = 0.2;

pub struct RuleEngine {
    rules: Vec<Rule>,
    rule_states: HashMap<String, RuleState>,
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleEngine {
    pub fn new() -> Self {
        Self {
            rules: vec![],
            rule_states: HashMap::new(),
        }
    }

    // ── Loading ──────────────────────────────────────────────

    pub fn load_rules(&mut self, rules: Vec<Rule>) {
        for rule in &rules {
            self.rule_states
                .entry(rule.id.0.clone())
                .or_insert_with(|| RuleState::new(&rule.id.0));
        }
        info!(count = rules.len(), "rule engine loaded rules");
        self.rules = rules;
    }

    // ── Hot reload ───────────────────────────────────────────
    //
    // Preserves in-flight state for rules that still exist by ID.
    // Cancels in-flight state for rules that were removed.

    pub fn hot_reload(&mut self, rules: Vec<Rule>) -> HotReloadReport {
        let new_ids: std::collections::HashSet<String> = rules
            .iter()
            .map(|r| r.id.0.clone())
            .collect();

        let old_ids: std::collections::HashSet<String> = self
            .rule_states
            .keys()
            .cloned()
            .collect();

        // Cancel in-flight for removed rules
        let mut in_flight_cancelled = 0u32;
        for removed_id in old_ids.difference(&new_ids) {
            if let Some(rs) = self.rule_states.remove(removed_id) {
                let count = rs.in_flight.len() as u32;
                if count > 0 {
                    warn!(
                        rule_id = %removed_id,
                        entries = count,
                        "rule removed — cancelling in-flight entries"
                    );
                    in_flight_cancelled += count;
                }
            }
        }

        let rules_removed = old_ids.difference(&new_ids).count() as u32;
        let rules_added   = new_ids.difference(&old_ids).count() as u32;

        // Add state for new rules
        for rule in &rules {
            self.rule_states
                .entry(rule.id.0.clone())
                .or_insert_with(|| RuleState::new(&rule.id.0));
        }

        let rules_loaded = rules.len() as u32;
        self.rules = rules;

        info!(
            rules_loaded,
            rules_added,
            rules_removed,
            in_flight_cancelled,
            "rule engine hot-reloaded"
        );

        HotReloadReport {
            rules_loaded,
            rules_rejected: 0, // loader handles rejection before we get here
            rules_added,
            rules_removed,
            in_flight_cancelled,
        }
    }

    // ── Evaluate — called on every StateUpdate ───────────────

    pub fn evaluate(
        &mut self,
        update: &StateUpdate,
        state: &HashMap<DeviceId, DeviceState>,
        now: SystemTime,
    ) -> Vec<Command> {   
        let now_ms = to_ms(now);
        let mut commands = vec![];

        for rule in self.rules.clone().iter() {
            if !self.is_triggered(rule, update) {
                continue;
            }

            debug!(rule_id = %rule.id, "trigger matched — evaluating conditions");

            if let Some(cmds) = self.evaluate_rule(rule, state, now_ms) {
                commands.extend(cmds);
            }
        }

        commands
    }

    // ── Tick — called every second ───────────────────────────
    //
    // Fires expired in-flight entries — delayed actions and timeouts.
    // Must be called even when there are no incoming events, which is
    // exactly when timeouts need to fire.

    pub fn tick(
        &mut self,
        now: SystemTime,
        state: &HashMap<DeviceId, DeviceState>,
    ) -> Vec<Command> {
        let now_ms = to_ms(now);
        let mut commands = vec![];

        // Build priority map before mutably borrowing rule_states
        let priority_map: HashMap<String, u8> = self.rules
            .iter()
            .map(|r| (r.id.0.clone(), r.priority))
            .collect();

        for rule_state in self.rule_states.values_mut() {
            let priority = priority_map.get(&rule_state.rule_id).copied().unwrap_or(0);
            let mut fired_indices = vec![];

            for (idx, entry) in rule_state.in_flight.iter().enumerate() {
                if now_ms >= entry.execute_at {
                    fired_indices.push(idx);
                    info!(
                        rule_id = %rule_state.rule_id,
                        kind = ?entry.kind,
                        "in-flight entry fired"
                    );
                    for action in &entry.actions {
                        commands.push(Command::new(
                            DeviceId(action.device_id.clone()),
                            AttributeKey(action.attribute.clone()),
                            action.value.clone(),
                            Some(RuleId(rule_state.rule_id.clone())),
                            priority,
                        ));
                    }
                }
            }

            // Remove fired entries in reverse order to preserve indices
            for idx in fired_indices.into_iter().rev() {
                rule_state.in_flight.remove(idx);
            }
        }

        let _ = state; // reserved for future condition re-evaluation on timeout
        commands
    }

    // ── UI summaries ─────────────────────────────────────────
    //
    // Read-only projections for the dashboard.
    // rule_summaries — stable, updated on load and hot-reload only.
    // in_flight_summaries — volatile, updated on every tick.

    pub fn rule_summaries(&self) -> Vec<RuleSummary> {
        self.rules
            .iter()
            .map(RuleSummary::from_rule)
            .collect()
    }

    pub fn in_flight_summaries(&self) -> Vec<InFlightSummary> {
        let mut summaries = vec![];

        for rule_state in self.rule_states.values() {
            for entry in &rule_state.in_flight {
                // Use the first action as the representative display row.
                // Most in-flight entries have one action. If multiple, the
                // dashboard shows the first — user reads the rule for the rest.
                if let Some(first) = entry.actions.first() {
                    summaries.push(InFlightSummary {
                        rule_id:    rule_state.rule_id.clone(),
                        device_id:  first.device_id.clone(),
                        attribute:  first.attribute.clone(),
                        value:      first.value.clone(),
                        fires_at_ms: entry.execute_at,
                        kind:       entry.kind.clone(),
                    });
                }
            }
        }

        // Sort by fires_at_ms ascending — soonest first in the UI
        summaries.sort_by_key(|s| s.fires_at_ms);
        summaries
    }

    // ── Trigger check ────────────────────────────────────────

    fn is_triggered(&self, rule: &Rule, update: &StateUpdate) -> bool {
        match &rule.trigger {
            Trigger::DeviceStateChanged { device_id, .. } => {
                update.changed_devices.contains(device_id)
            }
        }
    }

    // ── Rule evaluation ──────────────────────────────────────

    fn evaluate_rule(
        &mut self,
        rule: &Rule,
        state: &HashMap<DeviceId, DeviceState>,
        now_ms: u64,
    ) -> Option<Vec<Command>> {
        let mut all_met = true;

        {
            let rule_state = self.rule_states
                .entry(rule.id.0.clone())
                .or_insert_with(|| RuleState::new(&rule.id.0));

            for (idx, condition) in rule.conditions.iter().enumerate() {
                let device_state = state.get(&condition.device_id);
                let base_result = evaluate_condition_value(condition, device_state);

                if !base_result {
                    rule_state.condition_met_at.remove(&idx);
                    all_met = false;
                    break;
                }

                // Duration check
                if let Some(duration_secs) = condition.duration_seconds {
                    match rule_state.condition_met_at.get(&idx) {
                        None => {
                            rule_state.condition_met_at.insert(idx, now_ms);
                            debug!(
                                rule_id = %rule.id,
                                condition_idx = idx,
                                duration_secs,
                                "duration condition started"
                            );
                            all_met = false;
                            break;
                        }
                        Some(&met_at) => {
                            let elapsed_ms = now_ms.saturating_sub(met_at);
                            let required_ms = duration_secs * 1000;
                            if elapsed_ms < required_ms {
                                debug!(
                                    rule_id = %rule.id,
                                    elapsed_ms,
                                    required_ms,
                                    "duration condition pending"
                                );
                                all_met = false;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if !all_met {
            return None;
        }

        info!(rule_id = %rule.id, rule_name = %rule.name, "rule fired");

        // Clear duration timers after firing
        if let Some(rs) = self.rule_states.get_mut(&rule.id.0) {
            rs.condition_met_at.clear();
        }

        let mut immediate_commands = vec![];

        {
            let rule_state = self.rule_states
                .get_mut(&rule.id.0)
                .unwrap();

            for action in &rule.actions {
                if let Some(delay_secs) = action.delay_seconds {
                    // Schedule as delayed in-flight entry
                    let entry = InFlightEntry {
                        execute_at: now_ms + delay_secs * 1000,
                        actions: vec![SerializedAction::from_action(action)],
                        kind: InFlightKind::DelayedAction,
                    };
                    info!(
                        rule_id = %rule.id,
                        device_id = %action.device_id,
                        delay_secs,
                        "action scheduled as delayed in-flight"
                    );
                    rule_state.in_flight.push(entry);
                } else {
                    immediate_commands.push(Command::new(
                        action.device_id.clone(),
                        action.attribute.clone(),
                        action.value.clone(),
                        Some(rule.id.clone()),
                        rule.priority,
                    ));
                }
            }

            // Handle stateful timeout — reset if already running
            if let Some(stateful) = &rule.stateful {
                rule_state.clear_timeout();

                let timeout_entry = InFlightEntry {
                    execute_at: now_ms + stateful.timeout_seconds * 1000,
                    actions: stateful.on_timeout_actions
                        .iter()
                        .map(SerializedAction::from_action)
                        .collect(),
                    kind: InFlightKind::Timeout,
                };
                info!(
                    rule_id = %rule.id,
                    timeout_secs = stateful.timeout_seconds,
                    "stateful timeout set"
                );
                rule_state.in_flight.push(timeout_entry);
            }
        }

        Some(immediate_commands)
    }
}

// ── Condition evaluation (pure functions) ────────────────────

fn evaluate_condition_value(
    condition: &Condition,
    device_state: Option<&DeviceState>,
) -> bool {
    // IsUnknown — checks confidence, never reads actual
    if condition.operator == Operator::IsUnknown {
        return device_state
            .map(|s| s.confidence.value <= UNKNOWN_THRESHOLD)
            .unwrap_or(true);
    }

    let ds = match device_state {
        Some(s) => s,
        None => return false,
    };

    // WasPreviously — reads previous, not actual or get_effective
    if condition.operator == Operator::WasPreviously {
        let prev = ds.previous.get(&condition.attribute);
        return match &condition.value {
            ConditionValue::Single(expected) => prev == Some(expected),
            _ => false,
        };
    }

    // All other operators — read via get_effective.
    // Returns None when confidence is low and no safe default exists —
    // condition is unevaluable, rule must not fire.
    let effective = ds.get_effective(&condition.attribute, UNKNOWN_THRESHOLD);

    match &condition.operator {
        Operator::Equals => {
            match &condition.value {
                ConditionValue::Single(v) => effective == Some(v),
                _ => false,
            }
        }
        Operator::NotEquals => {
            match &condition.value {
                ConditionValue::Single(v) => effective != Some(v),
                _ => false,
            }
        }
        Operator::GreaterThan => {
            match (effective, &condition.value) {
                (Some(Value::Float(a)), ConditionValue::Single(Value::Float(b))) => a > b,
                (Some(Value::Int(a)),   ConditionValue::Single(Value::Int(b)))   => a > b,
                (Some(Value::Float(a)), ConditionValue::Single(Value::Int(b)))   => *a > *b as f64,
                _ => false,
            }
        }
        Operator::LessThan => {
            match (effective, &condition.value) {
                (Some(Value::Float(a)), ConditionValue::Single(Value::Float(b))) => a < b,
                (Some(Value::Int(a)),   ConditionValue::Single(Value::Int(b)))   => a < b,
                (Some(Value::Float(a)), ConditionValue::Single(Value::Int(b)))   => *a < *b as f64,
                _ => false,
            }
        }
        Operator::Between => {
            match (effective, &condition.value) {
                (
                    Some(Value::Float(a)),
                    ConditionValue::Range(Value::Float(lo), Value::Float(hi))
                ) => a >= lo && a <= hi,
                (
                    Some(Value::Int(a)),
                    ConditionValue::Range(Value::Int(lo), Value::Int(hi))
                ) => a >= lo && a <= hi,
                _ => false,
            }
        }
        Operator::Changed => true,
        Operator::WasPreviously | Operator::IsUnknown => unreachable!(),
    }
}

// ── Helpers ──────────────────────────────────────────────────

fn to_ms(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}