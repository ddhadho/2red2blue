use async_trait::async_trait;
use kernel::types::{RawDeviceEvent, AdapterCommand};

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("disconnected")]
    Disconnected,

    #[error("timeout")]
    Timeout,

    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("command rejected: {0}")]
    CommandRejected(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("not connected: {0}")]
    NotConnected(String),
}

#[async_trait]
pub trait DeviceAdapter: Send + Sync {
    async fn connect(&mut self) -> Result<(), AdapterError>;
    async fn disconnect(&mut self) -> Result<(), AdapterError>;
    fn is_connected(&self) -> bool;
    async fn next_event(&mut self) -> Result<RawDeviceEvent, AdapterError>;
    async fn send_command(&mut self, command: AdapterCommand) -> Result<(), AdapterError>;
}