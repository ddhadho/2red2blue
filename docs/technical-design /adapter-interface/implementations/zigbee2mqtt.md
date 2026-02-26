Zigbee2MQTT implementation 

## Zigbee2MQTT publishes device events to MQTT topics and receives commands the same way.

```rust
struct Zigbee2MqttAdapter {
    mqtt_url: String,
    client: MqttClient,
}

impl DeviceAdapter for Zigbee2MqttAdapter {
    async fn next_event(&mut self) -> Result<RawDeviceEvent, AdapterError> {
        // Subscribe to zigbee2mqtt/+/# topics
        // Parse MQTT message → RawDeviceEvent
        // Same output type as HA adapter
    }
    // ... same interface, different transport
}
```

