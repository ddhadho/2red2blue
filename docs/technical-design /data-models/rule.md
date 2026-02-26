# Rule Data Model

The `Rule` data model defines the structure for automation rules within the system, encompassing triggers, conditions, actions, and stateful behaviors.

## Rule Struct Definition

```rust
struct Rule {
    id: RuleId,
    name: String,
    enabled: bool,
    priority: u8,              // 0 = lowest, 255 = highest
    conflict_group: Option<String>,  // rules in same group compete

    trigger: Trigger,          // what starts evaluation
    conditions: Vec<Condition>, // what must be true
    actions: Vec<Action>,      // what to do

    stateful: Option<StatefulConfig>,  // if rule tracks in-flight state
}
```

## Rule Components

### Trigger Enum

```rust
enum Trigger {
    DeviceStateChanged { device_id: DeviceId, attribute: AttributeKey },
    TimeOfDay(CronExpression),
    SystemEvent(EventKind),
    Manual,
}
```

### Condition Struct

```rust
struct Condition {
    subject: ConditionSubject,
    operator: Operator,
    value: AttributeValue,
    duration: Option<Duration>,  // "has been true for X"
}
```

### Operator Enum

```rust
enum Operator {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    Changed,
}
```

### Action Struct

```rust
struct Action {
    device_id: DeviceId,
    attribute: AttributeKey,
    value: AttributeValue,
    delay: Option<Duration>,
}
```

### StatefulConfig Struct

```rust
struct StatefulConfig {
    timeout: Duration,        // how long before in-flight state expires
    on_timeout: Vec<Action>,  // what to do if timeout reached
}
```

## Key Features and Behavior

*   **`duration` in Condition:** The `duration` field on a `Condition` allows expressing "this has been true for X time" — addressing a common limitation in home automation systems. The rule engine tracks when a condition first became true and only fires the rule when the duration is satisfied.
*   **`stateful` Configuration:** The `stateful` configuration handles in-flight automations, such as "Motion detected → lights on for 10 minutes." The rule tracks that it fired, and after the defined timeout, it triggers the `on_timeout` actions. Crucially, if the daemon crashes and restarts, WAL replay reconstructs the in-flight state, allowing the timer to resume correctly without losing context.
