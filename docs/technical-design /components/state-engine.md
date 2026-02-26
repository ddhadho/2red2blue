# The world model. Owns all DeviceState. The only component allowed to write DeviceState.

## Trait Definition

```rust
trait StateEngine {
    // called by event ingestor after WAL append
    fn apply_event(&mut self, event: &Event) -> StateUpdate;

    // called by reconciler and rule engine
    fn get_device_state(&self, id: &DeviceId) -> Option<&DeviceState>;
    fn get_all_states(&self) -> &HashMap<DeviceId, DeviceState>;

    // called by rule engine when it sets desired state
    fn set_desired(&mut self, id: &DeviceId, attribute: AttributeKey,
                   value: AttributeValue, source: EventSource) -> Result<(), StateError>;

    // called by reconciler on boot
    fn apply_snapshot(&mut self, snapshot: Snapshot);

    // called continuously by confidence monitor
    fn tick(&mut self, now: SystemTime) -> Vec<DeviceId>; // returns degraded devices
}
```

## Method Explanations

*   **`tick` method:** Called every second. It checks every device's `last_seen` against `decays_after`. Devices past their decay threshold are returned so the reconciler can poll them. This is your confidence decay in action.
*   **`apply_event` method:** This is the hot path. It receives a normalized event, updates `actual` state for the relevant device, resets confidence to 1.0, and updates `last_seen`. It returns a `StateUpdate` that tells the rule engine which devices changed.

## StateUpdate Struct Definition

```rust
struct StateUpdate {
    changed_devices: Vec<DeviceId>,
    confidence_degraded: Vec<DeviceId>,
}
```
