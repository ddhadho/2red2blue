use std::collections::HashMap;
use std::time::SystemTime;
use tracing::{info, warn};
use crate::types::*;
use crate::registry::DeviceRegistry;

pub struct StateEngine {
    devices: HashMap<DeviceId, DeviceState>,
    degraded_threshold: f32,
    unknown_threshold: f32,
}

impl StateEngine {
    pub fn new(
        registry: &DeviceRegistry,
        degraded_threshold: f32,
        unknown_threshold: f32,
    ) -> Self {
        // Build DeviceState for every known device from the registry.
        // After this point the state engine holds everything it needs —
        // the registry is not retained.
        let mut devices = HashMap::new();

        for device in registry.all() {
            let confidence = Confidence::new(
                std::time::Duration::from_secs(device.confidence_decay_seconds),
                device.safe_default.clone(),
            );
            devices.insert(
                device.id.clone(),
                DeviceState::new(device.id.clone(), confidence),
            );
        }

        Self {
            devices,
            degraded_threshold,
            unknown_threshold,
        }
    }

    // ── Apply event ──────────────────────────────────────────

    pub fn apply_event(&mut self, event: &Event) -> StateUpdate {
        let mut changed = vec![];

        if let EventKind::DeviceStateChanged = &event.kind {
            if let EventSource::Device(device_id) = &event.source {
                let attribute = event.payload
                    .get("attribute")
                    .and_then(|v| if let Value::Text(s) = v {
                        Some(AttributeKey(s.clone()))
                    } else { None });

                let value = event.payload.get("value").cloned();

                if let (Some(attr), Some(val)) = (attribute, value) {
                    if let Some(state) = self.devices.get_mut(device_id) {
                        let now = SystemTime::now();
                        state.update_actual(attr.clone(), val.clone(), now);
                        changed.push(device_id.clone());

                        info!(
                            device_id = %device_id,
                            attribute = %attr,
                            value = %val,
                            confidence = state.confidence.value,
                            "state updated"
                        );
                    } else {
                        warn!(
                            device_id = %device_id,
                            "received event for unknown device — ignoring"
                        );
                    }
                }
            }
        }

        StateUpdate {
            changed_devices: changed,
        }
    }

    // ── Tick — called every second ───────────────────────────
    //
    // Takes `now` as a parameter for testability — no internal clock calls.
    // Does NOT write safe defaults into actual. Safe defaults are applied
    // at read time via get_effective.
    // Warns only on the transition into unknown_threshold, not every tick.

    pub fn tick(&mut self, now: SystemTime) -> Vec<DeviceId> {
        let mut degraded = vec![];

        for (device_id, state) in &mut self.devices {
            // Never seen — confidence already 0.0, nothing to decay
            if state.last_seen == SystemTime::UNIX_EPOCH {
                continue;
            }

            let age = now
                .duration_since(state.last_seen)
                .unwrap_or_default();

            let decays_after = state.confidence.decays_after;

            // Within grace period — full confidence, nothing to do
            if age <= decays_after {
                continue;
            }

            // Linear decay from 1.0 to 0.0 between decays_after and 2x decays_after
            let decay_progress = (age - decays_after).as_secs_f32()
                / decays_after.as_secs_f32();
            let new_confidence = (1.0 - decay_progress).max(0.0);

            if (new_confidence - state.confidence.value).abs() < f32::EPSILON {
                continue;
            }

            let was_above_unknown = state.confidence.value > self.unknown_threshold;
            let now_at_unknown    = new_confidence <= self.unknown_threshold;

            // Warn only on the transition — not on every tick below the threshold
            if was_above_unknown && now_at_unknown {
                warn!(
                    device_id = %device_id,
                    confidence = new_confidence,
                    "confidence crossed unknown threshold \
                     — safe defaults will apply via get_effective"
                );
            }

            state.confidence.value = new_confidence;

            if new_confidence <= self.degraded_threshold {
                degraded.push(device_id.clone());
            }
        }

        degraded
    }

    // ── Zero confidence ──────────────────────────────────────
    //
    // Called by main when a command fails after max retries.
    // Bypasses normal decay — sets confidence to 0.0 immediately.
    // get_effective switches to safe defaults on next read.
    // Any IsUnknown rule on this device fires on next state update.

    pub fn zero_confidence(&mut self, device_id: &DeviceId) {
        if let Some(state) = self.devices.get_mut(device_id) {
            state.zero_confidence();
            warn!(
                device_id = %device_id,
                "confidence zeroed — command failed after max retries"
            );
        }
    }

    // ── Desired state ────────────────────────────────────────

    pub fn set_desired(
        &mut self,
        device_id: &DeviceId,
        attribute: AttributeKey,
        value: Value,
        source: EventSource,
    ) {
        if let Some(state) = self.devices.get_mut(device_id) {
            state.desired.insert(attribute.clone(), value.clone());
            state.desired_set_at = SystemTime::now();
            state.desired_set_by = source;

            info!(
                device_id = %device_id,
                attribute = %attribute,
                value = %value,
                "desired state set"
            );
        } else {
            warn!(
                device_id = %device_id,
                "set_desired called for unknown device — ignoring"
            );
        }
    }

    // ── Diff — desired vs actual ─────────────────────────────
    //
    // Compares desired against actual for every device.
    // Each mismatch carries confidence so the reconciler can decide
    // whether to act immediately or poll first.

    pub fn diff(&self) -> Vec<StateMismatch> {
        let mut mismatches = vec![];

        for (device_id, state) in &self.devices {
            for (attr, desired_val) in &state.desired {
                let actual_val = state.actual.get(attr);
                if actual_val != Some(desired_val) {
                    mismatches.push(StateMismatch {
                        device_id: device_id.clone(),
                        attribute: attr.clone(),
                        desired: desired_val.clone(),
                        actual: actual_val.cloned(),
                        confidence: state.confidence.value,
                    });
                }
            }
        }

        mismatches
    }

    // ── Accessors ────────────────────────────────────────────

    pub fn get_all(&self) -> &HashMap<DeviceId, DeviceState> {
        &self.devices
    }

    pub fn get(&self, id: &DeviceId) -> Option<&DeviceState> {
        self.devices.get(id)
    }
}