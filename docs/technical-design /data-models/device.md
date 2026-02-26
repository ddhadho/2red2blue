# Device Data Model

The `Device` data model represents a physical or virtual device integrated into the smart home system, detailing its identity, type, and functionalities.

## Device Struct Definition

```rust
struct Device {
    id: DeviceId,                        // stable internal ID
    external_id: String,                 // HA entity_id or zigbee address
    name: String,                        // human readable
    kind: DeviceKind,                    // gate, light, pump, alarm, camera...
    capabilities: Vec<Capability>,       // what it can do and report
}
```

## DeviceKind Enum Definition

```rust
enum DeviceKind {
    Gate,
    SecurityLight,
    BoreholePump,
    WaterTank,
    AlarmPanel,
    Camera,
    SmartPlug,
    Inverter,
    Generator,
    PowerMonitor,    // detects KPLC state
    Sensor(SensorKind),
}
```

## Capability Enum Definition

```rust
enum Capability {
    Readable(AttributeKey),   // can report state
    Writable(AttributeKey),   // can receive commands
}
```

## Capability Model Explanation

The capability model is important. A security light might be writable (on/off command) but also readable (reports current state). A water tank sensor is only readable. A camera is readable (motion detected) but not writable through our system. This prevents the rule engine from trying to send commands to devices that can't receive them.