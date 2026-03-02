# Rule Data Model

The `Rule` data model defines the structure for automation rules within the system, encompassing triggers, conditions, actions, and stateful behaviors.

## Rule Struct Definition

```rust
struct Rule {
    id: RuleId,
    name: String,
    enabled: bool,
    priority: u8,                    // 0 = lowest, 255 = highest
    conflict_group: Option<String>,  // rules in same group compete

    trigger: Trigger,                // what starts evaluation
    conditions: Vec<Condition>,      // what must all be true
    actions: Vec<Action>,            // what to do

    stateful: Option<StatefulConfig>, // if rule tracks in-flight state
}
```

## Rule Components

### Trigger

```rust
enum Trigger {
    DeviceStateChanged { device_id: DeviceId, attribute: AttributeKey },
    TimeOfDay(CronExpression),
    SystemEvent(EventKind),
    Manual,
}
```

### Condition

```rust
struct Condition {
    subject: ConditionSubject,
    operator: Operator,
    value: ConditionValue,           // single value or range
    duration: Option<Duration>,      // "has been true for X seconds"
}

enum ConditionSubject {
    DeviceAttribute { device_id: DeviceId, attribute: AttributeKey },
    TimeOfDay,
}

enum ConditionValue {
    Single(Value),
    Range(Value, Value),             // for Between operator
}
```

### Operator

```rust
enum Operator {
    Equals,          // exact match against actual or get_effective
    NotEquals,       // inverse match
    GreaterThan,     // numeric comparison
    LessThan,        // numeric comparison
    Between,         // range check — works for numbers and time
    Changed,         // any change regardless of new value
    WasPreviously,   // checks DeviceState::previous before current change
    IsUnknown,       // confidence <= unknown_threshold for this attribute
}
```

`WasPreviously` reads `DeviceState::previous` — the value the attribute held immediately
before the most recent update. If the attribute has no previous value (never been reported
before this event), `WasPreviously` evaluates to false.

`IsUnknown` does not read `actual`. It checks whether `get_effective` would return a safe
default or `None` — i.e. whether confidence is at or below `unknown_threshold`. Use this
to write safety rules that fire when a device goes dark.

`Equals`, `NotEquals`, `GreaterThan`, `LessThan`, `Between` all read via `get_effective`.
If `get_effective` returns `None`, the condition is unevaluable and the rule does not fire.

`Changed` fires if the attribute appears in `StateUpdate::changed_devices` for the trigger
device. It does not inspect the value.

### Action

```rust
struct Action {
    device_id: DeviceId,
    attribute: AttributeKey,
    value: Value,
    delay: Option<Duration>,         // schedule this action N seconds after rule fires
}
```

Actions with `delay` are scheduled as in-flight entries at rule fire time and executed when
their timer expires. See In-Flight State below.

### StatefulConfig

```rust
struct StatefulConfig {
    timeout: Duration,               // how long before in-flight state expires
    on_timeout: Vec<Action>,         // what to do if timeout reached without reset
}
```

## In-Flight State

Both action-level `delay` and rule-level `StatefulConfig` timeout use the same timer
mechanism — the in-flight state system. When a rule fires:

- Each action with `delay` creates an in-flight entry with `execute_at = now + delay`
- If the rule has `StatefulConfig`, a timeout entry is created with `execute_at = now + timeout`

On every tick, the rule engine checks all in-flight entries. When `now >= execute_at`, the
actions are dispatched and the entry is removed.

```rust
struct RuleState {
    rule_id: RuleId,
    condition_met_at: Option<SystemTime>,  // for duration conditions
    in_flight: Vec<InFlightEntry>,
}

struct InFlightEntry {
    execute_at: SystemTime,
    actions: Vec<Action>,
    kind: InFlightKind,
}

enum InFlightKind {
    DelayedAction,   // from action.delay
    Timeout,         // from StatefulConfig
}
```

### WAL Durability

All in-flight entries are written to the WAL when created. On boot, WAL replay reconstructs
the in-flight map. Entries whose `execute_at` is in the past fire immediately on boot.
Entries still in the future resume their countdown from their original `execute_at` —
not from boot time.

This means a 30-second pump delay started at t=0, with the daemon crashing at t=15 and
restarting at t=20, fires the pump command at t=30 as intended.

### Timeout Reset

If a rule with `StatefulConfig` fires again before its timeout expires, the existing timeout
entry is replaced with a new one. The timer resets to `now + timeout`. This is the motion
sensor pattern — lights stay on as long as motion keeps being detected.

## Hot-Reload Contract

Rules are reloaded at runtime via SIGUSR1 without restarting the daemon. The contract for
in-flight state during reload:

- In-flight entries for rules that still exist with the same `id` are preserved unchanged
- In-flight entries for rules that were deleted or whose `id` changed are cancelled and logged
- New rules take effect immediately for future trigger firings only

Rule IDs must be stable across edits. Changing a rule's `id` is treated as deletion of the
old rule and addition of a new one — any in-flight state for the old ID is cancelled.

To cancel a running timer deliberately: disable the rule in TOML and reload. The rule still
exists by ID so its in-flight state is preserved on reload, but it will not fire new timers.
To cancel immediately, delete the rule or change its ID before reloading.