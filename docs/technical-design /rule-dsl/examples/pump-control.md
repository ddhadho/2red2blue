# Example: Water Pump Control with Duration Condition

These rules demonstrate how to control a water pump based on tank levels, utilizing a `duration_seconds` condition to prevent rapid cycling due to sensor fluctuations.

## Rule: Water Tank Low - Start Pump (TOML)

```toml
[[rules]]
id = "rule_004"
name = "Water tank low - start pump"
enabled = true
priority = 60
conflict_group = "pump"

[rules.trigger]
kind = "DeviceStateChanged"
device_id = "water_tank_sensor"
attribute = "level_percent"

[[rules.conditions]]
subject = { device_id = "water_tank_sensor", attribute = "level_percent" }
operator = "LessThan"
value = 30
duration_seconds = 60

[[rules.actions]]
device_id = "borehole_pump"
attribute = "state"
value = "on"
```

## Rule: Water Tank Full - Stop Pump (TOML)

```toml
[[rules]]
id = "rule_005"
name = "Water tank full - stop pump"
enabled = true
priority = 60
conflict_group = "pump"

[rules.trigger]
kind = "DeviceStateChanged"
device_id = "water_tank_sensor"
attribute = "level_percent"

[[rules.conditions]]
subject = { device_id = "water_tank_sensor", attribute = "level_percent" }
operator = "GreaterThan"
value = 95

[[rules.actions]]
device_id = "borehole_pump"
attribute = "state"
value = "off"
```

## Explanation: `duration_seconds`

The `duration_seconds = 60` on the low tank condition means the water tank must remain below 30% for a full minute before the pump starts. This crucial feature prevents the pump from cycling on and off rapidly due to noisy or fluctuating sensor readings, thereby extending the pump's lifespan and ensuring stable operation.