use std::collections::HashMap;
use crate::types::*;
use tracing::info;

pub struct StateEngine {
    devices: HashMap<DeviceId, DeviceState>,
}

impl StateEngine {
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
        }
    }

    pub fn apply_event(&mut self, event: &Event) -> StateUpdate {
        let mut changed = vec![];

        if let EventKind::DeviceStateChanged = event.kind {
            if let Some(EventSource::Device(device_id)) = Some(&event.source) {
                let attribute_key = event.payload
                    .get("attribute")
                    .and_then(|v| if let Value::Text(s) = v { 
                        Some(AttributeKey(s.clone())) 
                    } else { None });

                let value = event.payload.get("value").cloned();

                if let (Some(attr), Some(val)) = (attribute_key, value) {
                    let state = self.devices
                        .entry(device_id.clone())
                        .or_insert_with(|| DeviceState::new(device_id.clone()));

                    state.update_actual(attr.clone(), val.clone());
                    changed.push(device_id.clone());

                    info!(
                        device_id = %device_id,
                        attribute = %attr.0,
                        value = %val,
                        confidence = state.confidence,
                        "state updated"
                    );
                }
            }
        }

        StateUpdate {
            changed_devices: changed,
            confidence_degraded: vec![],
        }
    }

    pub fn get_all(&self) -> &HashMap<DeviceId, DeviceState> {
        &self.devices
    }
}