# Device State Data Model

The `DeviceState` data model represents the current understanding of a device's state within the system, maintained by the state engine.

## DeviceState Struct Definition

```rust
struct DeviceState {
    device_id: DeviceId,

    // what we want the device to be
    desired: HashMap<AttributeKey, AttributeValue>,

    // what the device last told us it is
    actual: HashMap<AttributeKey, AttributeValue>,

    // how much we trust actual reflects reality right now
    confidence: Confidence,

    // when we last heard from this device
    last_seen: SystemTime,

    // when desired was last set and by what
    desired_set_at: SystemTime,
    desired_set_by: EventSource,
}
```

## Confidence Struct Definition

```rust
struct Confidence {
    value: f32,          // 0.0 = unknown, 1.0 = certain
    decays_after: Duration,   // how long before we start doubting
    safe_default: HashMap<AttributeKey, AttributeValue>,  // what to assume when uncertain
}
```

## Confidence Decay Mechanism

The confidence decay is what makes this model honest about uncertainty. When a device reports state, confidence goes to `1.0`. If we haven't heard from it in `decays_after` duration, confidence starts dropping. Below a threshold — say `0.5` — the reconciler flags this device for a poll. Below `0.2`, the system applies the safe default.

Each device kind has a different `decays_after` — a gate you want to know about within 30 seconds. A water tank level you can tolerate not knowing for 5 minutes.
