use std::collections::HashMap;
use tracing::debug;
use crate::types::*;
use crate::registry::DeviceRegistry;

pub struct EventIngestor {
    registry: std::sync::Arc<DeviceRegistry>,
    sequence: u64,
    // Dedup: track last event per device
    last_events: HashMap<DeviceId, (u64, Value)>, // (timestamp, value)
    dedup_window_ms: u64,
}

impl EventIngestor {
    pub fn new(
        registry: std::sync::Arc<DeviceRegistry>,
        dedup_window_ms: u64,
    ) -> Self {
        Self {
            registry,
            sequence: 0,
            last_events: HashMap::new(),
            dedup_window_ms,
        }
    }

    pub fn ingest(&mut self, raw: RawDeviceEvent) -> Option<Event> {
        // Resolve external_id to internal DeviceId
        let device_id = self.registry
            .resolve_external(&raw.external_id)?
            .clone();

        // Dedup — drop if same value within dedup window
        if let Some((last_ts, last_val)) = self.last_events.get(&device_id) {
            let age_ms = raw.timestamp.saturating_sub(*last_ts);
            if age_ms < self.dedup_window_ms && last_val == &raw.value {
                debug!(
                    device_id = %device_id,
                    "duplicate event dropped"
                );
                return None;
            }
        }

        // Update dedup tracker
        self.last_events.insert(
            device_id.clone(),
            (raw.timestamp, raw.value.clone()),
        );

        self.sequence += 1;

        let mut payload = HashMap::new();
        payload.insert(
            "attribute".to_string(),
            Value::Text(raw.attribute.clone()),
        );
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

        debug!(sequence = event.sequence, "event ingested");

        Some(event)
    }
}