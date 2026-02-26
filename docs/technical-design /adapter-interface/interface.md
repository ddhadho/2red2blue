Contains only the trait and its responsibilities.


```rust
trait DeviceAdapter {
    // Lifecycle
    async fn connect(&mut self) -> Result<(), AdapterError>;
    async fn disconnect(&mut self) -> Result<(), AdapterError>;
    fn is_connected(&self) -> bool;

    // Inbound — device events coming in
    async fn next_event(&mut self) -> Result<RawDeviceEvent, AdapterError>;

    // Outbound — commands going out
    async fn send_command(&mut self, command: AdapterCommand) 
        -> Result<(), AdapterError>;

    // Discovery — what devices exist
    async fn list_devices(&self) -> Result<Vec<AdapterDevice>, AdapterError>;
    
    // Polling — ask a device for its current state
    async fn poll_device(&self, external_id: &str) 
        -> Result<RawDeviceEvent, AdapterError>;
}
```

**Four concerns** — lifecycle, inbound events, outbound commands, and device discovery. 