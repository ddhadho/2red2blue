# Device State Data Model

The `DeviceState` data model represents the current understanding of a device's state within the system, maintained by the state engine.

## DeviceState Struct Definition

```rust
struct DeviceState {
    device_id: DeviceId,

    // what we want the device to be
    desired: HashMap<AttributeKey, Value>,

    // what the device last told us it is — never overwritten by assumptions
    actual: HashMap<AttributeKey, Value>,

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
    value: f32,                                        // 0.0 = unknown, 1.0 = certain
    decays_after: Duration,                            // grace period before decay begins
    safe_default: HashMap<AttributeKey, Value>,        // what to act on when uncertain
}
```

## Confidence Decay Mechanism

When a device reports state, confidence resets to `1.0`. If the device goes silent, confidence
stays at `1.0` for `decays_after` — the grace period. After that it decays linearly, reaching
`0.0` at `2x decays_after`.

Two thresholds drive downstream behaviour:

| Constant | Value | Effect |
|---|---|---|
| `DEGRADED_THRESHOLD` | `0.5` | Reconciler flags device for polling |
| `UNKNOWN_THRESHOLD` | `0.2` | `get_effective` switches to safe default |

Each device has its own `decays_after` configured in `devices.toml`. A gate needs fresh state
within 30 seconds. A water tank can tolerate 5 minutes of silence.

## Reading Device State — `get_effective`

`actual` is a record of fact. It holds the last value the device genuinely reported and is
never overwritten by assumptions or safe defaults.

All components that need to *act* on device state — the rule engine, the reconciler, the diff —
call `get_effective` instead of reading `actual` directly. `get_effective` applies confidence
awareness at the point of use:

```rust
impl DeviceState {
    pub fn get_effective(&self, attr: &AttributeKey) -> Option<&Value> {
        if self.confidence.value <= UNKNOWN_THRESHOLD {
            self.confidence.safe_default.get(attr)
        } else {
            self.actual.get(attr)
        }
    }
}
```

Return behaviour:

| Condition | Returns |
|---|---|
| Confidence above `UNKNOWN_THRESHOLD` | Value from `actual` |
| Confidence at or below `UNKNOWN_THRESHOLD`, safe default defined | Safe default value |
| Confidence at or below `UNKNOWN_THRESHOLD`, no safe default | `None` |

When `None` is returned the caller knows it has no trustworthy value for that attribute.
The rule engine must handle this explicitly — a rule whose condition attribute returns `None`
is unevaluable and must not fire.

## Desired vs Actual — `diff`

The diff compares desired against actual, not against `get_effective`. It is a record of
intent vs last known fact. Each mismatch carries the confidence at time of diff so the
reconciler can decide whether to act immediately or poll first:

```rust
struct StateMismatch {
    device_id: DeviceId,
    attribute: AttributeKey,
    desired: Value,
    actual: Option<Value>,    // None if device has never reported this attribute
    confidence: f32,
}
```

Interpreting a mismatch:

| Confidence | Reconciler action |
|---|---|
| High (`>= DEGRADED_THRESHOLD`) | Actual is trustworthy — dispatch correction command |
| Low (`< DEGRADED_THRESHOLD`) | Actual is stale — poll device first, re-evaluate after |
| `actual` is `None` | Device has never reported — poll, do not dispatch |