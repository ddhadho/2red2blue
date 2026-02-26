use async_trait::async_trait;
use kernel::types::{RawDeviceEvent, AdapterCommand};

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Disconnected")]
    Disconnected,
    
    #[error("Timeout")]
    Timeout,
    
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    
    #[error("Command rejected: {0}")]
    CommandRejected(String),
    
    #[error("Parse error: {0}")]
    ParseError(String),
}

#[async_trait]
pub trait DeviceAdapter: Send + Sync {
    async fn connect(&mut self) -> Result<(), AdapterError>;
    async fn disconnect(&mut self) -> Result<(), AdapterError>;
    fn is_connected(&self) -> bool;
    async fn next_event(&mut self) -> Result<RawDeviceEvent, AdapterError>;
    async fn send_command(&mut self, command: AdapterCommand) -> Result<(), AdapterError>;
}