use crate::types::Command;
use tracing::info;

// Stub — full implementation in Milestone 5
pub struct CommandDispatcher;

impl CommandDispatcher {
    pub fn new() -> Self { Self }

    pub fn dispatch(&self, command: &Command) {
        info!(
            command_id = %command.id,
            device_id = %command.device_id,
            attribute = %command.attribute.0,
            value = %command.value,
            "command dispatched (stub — not sent to real device)"
        );
    }
}