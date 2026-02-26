Triggers determine when a rule is evaluated.

## Supported Trigger Kinds

*   `DeviceStateChanged`: Fires when a device attribute changes.
*   `TimeOfDay`: Fires at specific times.
*   `ExternalEvent`: Custom future events (placeholder for extensibility).

## Trigger Structure

```toml
[rules.trigger]
kind = "DeviceStateChanged"
device_id = "motion_sensor_front"
attribute = "state"
```

## Behavior

Triggers are matched against events in the system. Multiple rules can share the same trigger; conflict resolution is handled later in the process.