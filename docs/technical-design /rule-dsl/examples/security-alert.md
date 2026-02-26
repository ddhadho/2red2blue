# Gate Opened at Night — Alert

When the main gate opens at night, trigger the alarm and front lights.

## Rule Definition (TOML)

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

## Rule Explanation

*   **Trigger:** The rule is triggered when the `gate_main` changes its `state`.
*   **Conditions:**
    *   The `gate_main`'s `state` must be `open`.
    *   The current time must be `Between` 10 PM (`22:00`) and 6 AM (`06:00`).
*   **Actions:**
    *   The `alarm_panel`'s `mode` is set to `alert`.
    *   The `security_lights_front` are turned `on`.
