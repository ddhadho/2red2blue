Stateful rules enable more complex automation scenarios, particularly those involving **timeout behavior**.

## TOML Example

```toml
[[rules.stateful]]
timeout_seconds = 600

[[rules.stateful.on_timeout_actions]]
device_id = "security_lights_front"
attribute = "state"
value = "off"
```

## Behavior and Durability

This example demonstrates a rule where `on_timeout_actions` are executed if the rule's trigger does not occur again within `timeout_seconds`. A common use case is a motion sensor triggering lights that then turn off after 10 minutes of no further motion.

Key aspects:

*   **WAL ensures timers resume correctly if daemon crashes:** The Write-Ahead Log stores the state of these timers, allowing them to be reconstructed and continue functioning correctly even if the daemon unexpectedly restarts.
*   **Timers reset if trigger occurs again before timeout:** If the triggering event happens again before the `timeout_seconds` have elapsed, the timer is reset, preventing premature execution of `on_timeout_actions`.
