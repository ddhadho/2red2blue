Rules are defined as TOML `[[rules]]` entries, encapsulating triggers, conditions, and actions.

## Complete Rule TOML Example

```toml
[[rules]]
id = "rule_001"
name = "Gate opened at night - alert"
enabled = true
priority = 80
conflict_group = "security"

[rules.trigger]
kind = "DeviceStateChanged"
device_id = "gate_main"
attribute = "state"

[[rules.conditions]]
subject = { device_id = "gate_main", attribute = "state" }
operator = "Equals"
value = "open"

[[rules.conditions]]
subject = { kind = "TimeOfDay" }
operator = "Between"
value = ["22:00", "06:00"]

[[rules.actions]]
device_id = "alarm_panel"
attribute = "mode"
value = "alert"

[[rules.actions]]
device_id = "security_lights_front"
attribute = "state"
value = "on"
```

## Explanation of Fields

*   **`id`**: A unique identifier for the rule.
*   **`name`**: A human-readable name for the rule.
*   **`enabled`**: Boolean, `true` if the rule is active, `false` otherwise.
*   **`priority`**: An integer indicating the rule's priority in conflict resolution (higher value = higher priority).
*   **`conflict_group`**: A string identifier for a group of rules that might conflict.
*   **`trigger`**: Defines the event that initiates the rule's evaluation.
*   **`conditions`**: A list of conditions that must all be met for the actions to execute.
*   **`actions`**: A list of actions to perform if all conditions are met.
