# Example: Power Recovery Automation

This example demonstrates a critical automation for handling power restoration, ensuring a safe and staggered recovery of devices after an outage.

## Rule Definition (TOML)

```toml
[[rules]]
id = "rule_002"
name = "Power restored - full recovery"
enabled = true
priority = 255
conflict_group = "power"

[rules.trigger]
kind = "DeviceStateChanged"
device_id = "power_monitor"
attribute = "source"

[[rules.conditions]]
subject = { device_id = "power_monitor", attribute = "source" }
operator = "Equals"
value = "kplc"

[[rules.conditions]]
subject = { device_id = "power_monitor", attribute = "source" }
operator = "WasPreviously"
value = "outage"
duration_seconds = 0

[[rules.actions]]
device_id = "gate_main"
attribute = "state"
value = "locked"

[[rules.actions]]
device_id = "borehole_pump"
attribute = "state"
value = "on"
delay_seconds = 30

[[rules.actions]]
device_id = "security_lights_front"
attribute = "state"
value = "auto"
```

## Rule Explanation

*   **Trigger:** The rule is triggered when the `power_monitor`'s `source` attribute changes.
*   **Conditions:**
    *   The `power_monitor`'s `source` must now be `kplc` (indicating utility power is restored).
    *   The `power_monitor`'s `source` must have `WasPreviously` `outage` (ensuring the rule only runs after an actual outage).
*   **Actions:**
    *   The `gate_main` is `locked` immediately.
    *   The `borehole_pump` is turned `on` after a `30-second delay`. This `delay_seconds` is crucial for staggered recovery, preventing all devices from activating simultaneously and potentially overloading the electrical circuit.
    *   `security_lights_front` are set to `auto` mode.
