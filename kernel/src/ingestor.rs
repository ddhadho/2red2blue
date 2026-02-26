use std::collections::HashMap;
use crate::types::*;
use tracing::debug;

pub struct NormalizationMap {
    entries: HashMap<String, (DeviceId, String)>,
}

impl NormalizationMap {
    pub fn new() -> Self {
        let mut entries = HashMap::new();
        // Hardcoded for walking skeleton — loaded from devices.toml in M3
        entries.insert(
            "switch.main_gate".to_string(),
            (DeviceId("gate_main".to_string()), "state".to_string()),
        );
        Self { entries }
    }

    pub fn normalize(&self, external_id: &str) -> Option<(DeviceId, String)> {
        self.entries.get(external_id).cloned()
    }
}

pub struct EventIngestor {
    norm_map: NormalizationMap,
    sequence: u64,
}

impl EventIngestor {
    pub fn new() -> Self {
        Self {
            norm_map: NormalizationMap::new(),
            sequence: 0,
        }
    }

    pub fn ingest(&mut self, raw: RawDeviceEvent) -> Option<Event> {
        let (device_id, attribute) = self.norm_map.normalize(&raw.external_id)?;

        self.sequence += 1;

        let mut payload = HashMap::new();
        payload.insert("attribute".to_string(), Value::Text(attribute.clone()));
        payload.insert("value".to_string(), raw.value.clone());
        payload.insert(
            "device_id".to_string(),
            Value::Text(device_id.0.clone()),
        );

        let mut event = Event::new(
            EventSource::Device(device_id),
            EventKind::DeviceStateChanged,
            payload,
        );
        event.sequence = self.sequence;

        debug!(
            sequence = event.sequence,
            "event ingested"
        );

        Some(event)
    }
}