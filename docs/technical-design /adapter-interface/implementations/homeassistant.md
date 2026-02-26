The HA adapter is the only file in the codebase that knows what a HA websocket message looks like. Everything it receives gets normalized before leaving this file. Everything it sends gets translated from our internal format inside this file.


```rust
struct HomeAssistantAdapter {
    url: String,
    token: String,
    ws: Option<WebSocketConnection>,
    pending_commands: HashMap<Ulid, oneshot::Sender<Result<(), AdapterError>>>,
}

impl DeviceAdapter for HomeAssistantAdapter {
    async fn connect(&mut self) -> Result<(), AdapterError> {
        // 1. Open websocket to ws://localhost:8123/api/websocket
        // 2. Authenticate with token
        // 3. Subscribe to all state_changed events
        // 4. Mark connected
    }

    async fn next_event(&mut self) -> Result<RawDeviceEvent, AdapterError> {
        // 1. Read next message from websocket
        // 2. If state_changed event → normalize to RawDeviceEvent
        // 3. If result message → match to pending command by id
        //    → resolve the oneshot sender
        // 4. If ping → send pong, loop back to next message
        // 5. If disconnect → return AdapterError::Disconnected
    }

    async fn send_command(&mut self, command: AdapterCommand) 
        -> Result<(), AdapterError> {
        // 1. Translate AdapterCommand to HA service call
        //    e.g. attribute="state", value=true → light.turn_on
        // 2. Send via websocket with unique message id
        // 3. Register in pending_commands map
        // 4. Return immediately — confirmation comes via next_event
    }

    async fn list_devices(&self) -> Result<Vec<AdapterDevice>, AdapterError> {
        // HTTP GET /api/states
        // Map each HA state object to AdapterDevice
    }

    async fn poll_device(&self, external_id: &str) 
        -> Result<RawDeviceEvent, AdapterError> {
        // HTTP GET /api/states/{entity_id}
        // Map to RawDeviceEvent
    }
}
```