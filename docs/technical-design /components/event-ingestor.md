# Event Ingestor

Sits at the boundary between the adapter and the system. Its job is normalization and
deduplication. This is the only component that knows what an external `entity_id` looks like.
Everything above this layer speaks the internal model only.

## Implementation

```rust
pub struct EventIngestor {
    registry: Arc<DeviceRegistry>,
    sequence: u64,
    last_events: HashMap<DeviceId, (u64, Value)>,
    dedup_window_ms: u64,
}
```

## Ingest

```rust
pub fn ingest(&mut self, raw: RawDeviceEvent) -> Option<Event>
```

Returns `None` if the event is a duplicate or the `external_id` is not in the registry.
Returns `Some(Event)` with a sequence number assigned ready for WAL append.

## Normalization

Normalization means mapping the adapter's `external_id` to an internal `DeviceId` via the
device registry. The registry is the single source of truth for this mapping — loaded from
`devices.toml` at startup.

Attribute names pass through unchanged. The raw event's `attribute` field is used directly
as the internal `AttributeKey`. This works because `devices.toml` is authored to match the
attribute names the current adapter (HA) reports.

## Deduplication

If the same device reports the same value within `dedup_window_ms`, the second event is
dropped. HA can be chatty — this prevents the state engine and WAL from processing
redundant updates.

The dedup window is configurable via `AdapterConfig.event_dedup_window_ms` in `config.toml`.
The default is 50ms.

The dedup tracker is keyed by `DeviceId`. This means the last reported value is tracked per
device regardless of which attribute changed. If two different attributes on the same device
change within the window, the second may be dropped if its value matches the first's tracked
value. This is acceptable for V1 where devices typically report one attribute at a time.

## System events and confirmations

The ingestor only handles device state reports. It resolves `external_id` against the
registry — any event whose `external_id` is not a registered device is dropped.

This means system events like command confirmations must never be routed through the
ingestor. They travel through a dedicated channel (`confirm_tx`) from the adapter directly
to main, which calls `dispatcher.confirm(command_id)` on receipt.

The ingestor has no knowledge of commands, confirmations, or system event kinds. That
boundary is intentional — keeping the ingestor focused on device observations makes it
simpler and prevents system events from accidentally being treated as state changes.

## V2 — Attribute Mapping

When HA is replaced with a native adapter, the adapter's attribute names may differ from
internal `AttributeKey` names. For example, a Zigbee adapter might report `lock_state` for
an attribute the system calls `state` internally.

The fix is an optional per-device attribute map in `devices.toml`:

```toml
[devices.attribute_map]
lock_state = "state"    # adapter attribute name → internal AttributeKey
```

When absent the ingestor passes attribute names through unchanged — V1 HA behaviour.
When present the ingestor translates before building the event — native adapter behaviour.
Nothing above the ingestor changes either way.

Deduplication should also be moved to a per `(DeviceId, AttributeKey)` key in V2 to
correctly handle devices that report multiple attributes.