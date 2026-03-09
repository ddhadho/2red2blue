use async_trait::async_trait;
use kernel::types::{RawDeviceEvent, AdapterCommand, Value};
use tokio::sync::mpsc;
use tracing::info;
use crate::traits::{AdapterError, DeviceAdapter};

pub struct MockAdapter {
    connected: bool,
    tick: u64,
    // Confirmation channel — sends command_id strings directly to main,
    // bypassing the ingestor. Ingestor only handles device state reports.
    confirm_tx: mpsc::Sender<String>,
}

impl MockAdapter {
    pub fn new(confirm_tx: mpsc::Sender<String>) -> Self {
        Self {
            connected: false,
            tick: 0,
            confirm_tx,
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
        // Simulation sequence:
        //
        // Ticks 1–4:   gate alternates open/closed, 5s apart. Normal operation.
        // Tick  5:     power outage — power monitor reports "outage".
        // Ticks 6–11:  silence on the gate. Power monitor keeps repeating
        //              "outage" every 10s to keep the event loop alive.
        //              gate.confidence_decay_seconds = 30, so:
        //                t=30s of silence → decay begins, confidence starts falling
        //                t=60s of silence → confidence = 0.0, safe default applies
        //              6 ticks × 10s = 60s — enough to reach confidence 0.0.
        // Tick  12:    power restored.
        // Ticks 13+:   gate resumes reporting — confidence resets to 1.0.

        let sleep_secs = match self.tick {
            5..=10 => 10,
            _      => 5,
        };

        tokio::time::sleep(tokio::time::Duration::from_secs(sleep_secs)).await;
        self.tick += 1;

        let event = match self.tick {
            5 => {
                info!("SIMULATION: power outage begins — gate will go silent");
                RawDeviceEvent {
                    external_id: "binary_sensor.mains_power".to_string(),
                    attribute: "source".to_string(),
                    value: Value::Text("outage".to_string()),
                    timestamp: now_ms(),
                    raw: serde_json::json!({"state": "outage"}),
                }
            }

            6..=11 => {
                info!(tick = self.tick, "SIMULATION: gate silent — power still out");
                RawDeviceEvent {
                    external_id: "binary_sensor.mains_power".to_string(),
                    attribute: "source".to_string(),
                    value: Value::Text("outage".to_string()),
                    timestamp: now_ms(),
                    raw: serde_json::json!({"state": "outage"}),
                }
            }

            12 => {
                info!("SIMULATION: power restored — gate will resume");
                RawDeviceEvent {
                    external_id: "binary_sensor.mains_power".to_string(),
                    attribute: "source".to_string(),
                    value: Value::Text("kplc".to_string()),
                    timestamp: now_ms(),
                    raw: serde_json::json!({"state": "kplc"}),
                }
            }

            _ => {
                let state = if self.tick % 2 == 0 { "open" } else { "closed" };
                RawDeviceEvent {
                    external_id: "switch.main_gate".to_string(),
                    attribute: "state".to_string(),
                    value: Value::Text(state.to_string()),
                    timestamp: now_ms(),
                    raw: serde_json::json!({
                        "entity_id": "switch.main_gate",
                        "state": state,
                    }),
                }
            }
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

    // Trait contract: fire and forget, returns Ok(()).
    // Confirmation bypasses the ingestor — sent directly as a command_id
    // string through confirm_tx. Main calls dispatcher.confirm() on receipt.
    async fn send_command(
        &mut self,
        command: AdapterCommand,
    ) -> Result<(), AdapterError> {
        info!(
            external_id = %command.external_id,
            attribute = %command.attribute,
            value = ?command.value,
            command_id = %command.command_id,
            "mock adapter received command — confirming immediately"
        );

        // Best-effort — if channel is full or closed, drop the confirmation.
        // Dispatcher will retry after timeout.
        self.confirm_tx.try_send(command.command_id).ok();

        Ok(())
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}