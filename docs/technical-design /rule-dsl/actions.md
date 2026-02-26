Actions define the operations executed when a rule's conditions are met.

## TOML Example

```toml
[[rules.actions]]
device_id = "alarm_panel"
attribute = "mode"
value = "alert"

[[rules.actions]]
device_id = "security_lights_front"
attribute = "state"
value = "on"
```

## Properties

*   `device_id`: The ID of the target device.
*   `attribute`: The attribute of the device to modify.
*   `value`: The value to set the attribute to.
*   `delay_seconds` (optional): Delays execution of this action by the specified number of seconds.
    Actions are executed sequentially.
    Logged to JSON structured logs for traceability.