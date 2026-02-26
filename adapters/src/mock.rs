use async_trait::async_trait;
use kernel::types::{RawDeviceEvent, AdapterCommand, Value};
use tracing::info;
use crate::traits::{AdapterError, DeviceAdapter};

pub struct MockAdapter {
    connected: bool,
    tick: u64,
}

impl MockAdapter {
    pub fn new() -> Self {
        Self {
            connected: false,
            tick: 0,
        }
    }
}

#[async_trait]
impl DeviceAdapter for MockAdapter {
    async fn connect(&mut self) -> Result<(), AdapterError> {
        self.connected = true;
        info!("mock adapter connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), AdapterError> {
        self.connected = false;
        info!("mock adapter disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn next_event(&mut self) -> Result<RawDeviceEvent, AdapterError> {
        // Emit one event every 5 seconds
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

        self.tick += 1;

        // Alternate gate between open and closed
        let state = if self.tick % 2 == 0 { "open" } else { "closed" };

        let event = RawDeviceEvent {
            external_id: "switch.main_gate".to_string(),
            attribute: "state".to_string(),
            value: Value::Text(state.to_string()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            raw: serde_json::json!({
                "entity_id": "switch.main_gate",
                "state": state,
            }),
        };

        info!(
            external_id = %event.external_id,
            attribute = %event.attribute,
            value = ?event.value,
            tick = self.tick,
            "mock event generated"
        );

        Ok(event)
    }

    async fn send_command(&mut self, command: AdapterCommand) -> Result<(), AdapterError> {
        info!(
            external_id = %command.external_id,
            attribute = %command.attribute,
            value = ?command.value,
            command_id = %command.command_id,
            "mock adapter received command (not sending to real device)"
        );
        Ok(())
    }
}