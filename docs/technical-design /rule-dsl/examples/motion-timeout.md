# Example: Motion-Activated Lights with Timeout

This example demonstrates a stateful rule that turns on security lights when motion is detected and automatically turns them off after a set period of inactivity.

## Rule Definition (TOML)

```toml
[[rules]]
id = "rule_003"
name = "Motion detected - security lights on"
enabled = true
priority = 70
conflict_group = "security_lights"

[rules.trigger]
kind = "DeviceStateChanged"
device_id = "motion_sensor_front"
attribute = "state"

[[rules.conditions]]
subject = { device_id = "motion_sensor_front", attribute = "state" }
operator = "Equals"
value = "detected"

[[rules.actions]]
device_id = "security_lights_front"
attribute = "state"
value = "on"

[rules.stateful]
timeout_seconds = 600

[[rules.stateful.on_timeout_actions]]
device_id = "security_lights_front"
attribute = "state"
value = "off"
```

## Rule Explanation

*   **Trigger:** The rule is triggered when the `motion_sensor_front` changes its `state` to `detected`.
*   **Condition:** It checks if the `motion_sensor_front`'s `state` is `detected`.
*   **Action:** If the condition is met, the `security_lights_front` are turned `on`.
*   **Stateful Behavior:** A `timeout_seconds` of 600 (10 minutes) is set. If no further motion is detected within this period, the `on_timeout_actions` are executed, turning the `security_lights_front` `off`.
*   **Resilience:** If motion is detected again before the timeout, the timer resets. The Write-Ahead Log (WAL) ensures that if the daemon crashes at any point, the in-flight state is reconstructed upon restart, and the timer continues correctly.