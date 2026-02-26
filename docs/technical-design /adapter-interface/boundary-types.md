

```rust
// Event coming in from the device world
struct RawDeviceEvent {
    external_id: String,        // HA entity_id, zigbee address, etc
    attribute: String,          // "state", "level", "temperature"
    value: RawValue,            // what changed
    timestamp: SystemTime,
    raw: serde_json::Value,     // original payload, kept for debugging
}

// Command going out to the device world  
struct AdapterCommand {
    external_id: String,
    attribute: String,
    value: RawValue,
    command_id: Ulid,           // for correlation when confirmation arrives
}

// Device discovered during list_devices
struct AdapterDevice {
    external_id: String,
    friendly_name: String,
    kind_hint: Option<String>,  // "light", "switch", "sensor" — hint only
    attributes: Vec<String>,    // what attributes this device exposes
}

enum RawValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Null,
}
```

The `raw` field on `RawDeviceEvent` keeps the original HA payload untouched. You never use it in logic but it's invaluable for debugging when a device behaves unexpectedly. Log it, don't process it.

`kind_hint` on `AdapterDevice` is a hint not a guarantee. HA says this device is a "light" — your device registry uses that as a starting point when you first discover a device, but a human confirms and sets the actual `DeviceKind` during setup. Never trust the hint for logic.