use crate::types::*;
use tracing::info;
use std::collections::HashMap;

// Stub — full implementation in Milestone 4
pub struct RuleEngine;

impl RuleEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn evaluate(
        &mut self,
        update: &StateUpdate,
        state: &HashMap<DeviceId, DeviceState>,
    ) -> Vec<Command> {
        // Hardcoded rule for walking skeleton:
        // if gate_main state == "open" → log alert
        let gate_id = DeviceId("gate_main".to_string());

        if update.changed_devices.contains(&gate_id) {
            if let Some(device_state) = state.get(&gate_id) {
                let state_key = AttributeKey("state".to_string());
                if let Some(Value::Text(s)) = device_state.actual.get(&state_key) {
                    if s == "open" {
                        info!(
                            device_id = %gate_id,
                            "RULE TRIGGERED: gate opened — alert"
                        );
                    }
                }
            }
        }

        // No real commands yet
        vec![]
    }
}