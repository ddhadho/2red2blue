To write tests for the entire daemon — WAL, state engine, rule engine, reconciler — without a single real device or a running HA instance. 

struct MockAdapter {
    events: VecDeque<RawDeviceEvent>,   // pre-loaded test events
    commands_received: Vec<AdapterCommand>,  // inspect in tests
    devices: Vec<AdapterDevice>,
}

impl DeviceAdapter for MockAdapter {
    async fn connect(&mut self) -> Result<(), AdapterError> {
        Ok(())  // always succeeds
    }

    async fn next_event(&mut self) -> Result<RawDeviceEvent, AdapterError> {
        // Return pre-loaded events in sequence
        // When empty, block until a test pushes a new event
        // or return AdapterError::Timeout after configured duration
    }

    async fn send_command(&mut self, command: AdapterCommand)
        -> Result<(), AdapterError> {
        self.commands_received.push(command.clone());
        // Optionally auto-generate confirmation event
        // so tests don't have to manually simulate device responses
        Ok(())
    }
}