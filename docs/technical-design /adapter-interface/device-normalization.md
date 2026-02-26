One piece of configuration that lives at the adapter boundary — mapping external device identifiers to the internal DeviceIds.

# devices.toml

[[devices]]
id = "gate_main"
external_id = "switch.main_gate"     # HA entity_id
name = "Main Gate"
kind = "Gate"
capabilities = ["state"]
confidence_decay_seconds = 30
safe_default = { state = "locked" }

[[devices]]
id = "borehole_pump"
external_id = "switch.borehole_pump"
name = "Borehole Pump"
kind = "BoreholePump"
capabilities = ["state"]
confidence_decay_seconds = 300
safe_default = { state = "off" }

[[devices]]
id = "water_tank_sensor"
external_id = "sensor.water_tank_level"
name = "Water Tank"
kind = "Sensor"
capabilities = ["level_percent"]
confidence_decay_seconds = 300
safe_default = { level_percent = 0 }

[[devices]]
id = "power_monitor"
external_id = "binary_sensor.mains_power"
name = "KPLC Power Monitor"
kind = "PowerMonitor"
capabilities = ["source"]
confidence_decay_seconds = 10
safe_default = { source = "unknown" }


This file is the translation table. `external_id` is what HA calls it. `id` is what your daemon calls it. Every rule references `id` only. 