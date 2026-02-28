# State Engine

The world model. Owns all `DeviceState`. The only component allowed to write `DeviceState`.

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
                   value: Value, source: EventSource) -> Result<(), StateError>;

    // called by reconciler on boot after WAL replay
    fn apply_snapshot(&mut self, snapshot: Snapshot);

    // called every second by the main loop
    fn tick(&mut self, now: SystemTime) -> Vec<DeviceId>;

    // called by reconciler and dispatcher to find actionable mismatches
    fn diff(&self) -> Vec<StateMismatch>;
}
```

## Method Explanations

**`apply_event`** — the hot path. Receives a normalized event from the ingestor, updates
`actual` for the relevant device, resets `confidence.value` to `1.0`, and updates `last_seen`.
Returns a `StateUpdate` telling the rule engine which devices changed. Never touches `desired`.

**`tick`** — called every second with the current time as a parameter (for testability). For
each device it computes age since `last_seen` and decays `confidence.value` accordingly.
Decay begins after `confidence.decays_after` and reaches `0.0` at `2x decays_after`. Returns
the list of devices whose confidence crossed `DEGRADED_THRESHOLD` this tick — the reconciler
uses this list to decide what to poll. Does not write safe defaults into `actual`.

**`set_desired`** — called by the rule engine when a rule fires. Records the desired value,
the time it was set, and the source. Returns an error if the device is unknown.

**`apply_snapshot`** — called by the reconciler on boot after WAL replay. Loads the last
known full state into the engine before continuous operation begins.

**`diff`** — compares `desired` against `actual` for every device and returns all mismatches.
Each mismatch carries the current confidence so the reconciler can decide whether to act
immediately or poll first. See `device-state.md` for mismatch interpretation.

## StateUpdate

`StateUpdate` is the return value of `apply_event`. It signals the rule engine — not the
reconciler. Confidence degradation is signalled separately via `tick`.

```rust
struct StateUpdate {
    changed_devices: Vec<DeviceId>,   // devices whose actual state changed this event
}
```

## Confidence Decay in tick

`tick` does not reach into the registry. All decay parameters live on `Confidence` inside
`DeviceState` — the state engine is self-contained after initialization. The registry is only
consulted when constructing `DeviceState` for the first time.

Decay behaviour:

| Age since last_seen | Confidence |
|---|---|
| `< decays_after` | `1.0` — full confidence, grace period |
| `decays_after` to `2x decays_after` | Linear decay from `1.0` to `0.0` |
| `>= 2x decays_after` | `0.0` — unknown |

When confidence crosses `UNKNOWN_THRESHOLD`, `tick` does not write safe defaults into `actual`.
Safe defaults are applied at read time via `get_effective`. `actual` is never modified by decay.

## Initialization

On startup the state engine is initialized from the device registry. For each registered
device a `DeviceState` is constructed with:

- `confidence.value` = `0.0` — unknown until first report
- `confidence.decays_after` = from `devices.toml`
- `confidence.safe_default` = from `devices.toml`
- `actual` = empty — no assumptions
- `desired` = empty — no intent until rules or user sets it