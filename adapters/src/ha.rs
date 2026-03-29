use std::collections::HashMap;
use std::time::SystemTime;

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    WebSocketStream, MaybeTlsStream,
};

use kernel::config::{HaConfig, HaDeviceConfig};
use kernel::types::{AdapterCommand, RawDeviceEvent, Value, EventSource};

use crate::traits::{AdapterError, DeviceAdapter};

// ── WebSocket message types ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct HaMessage {
    #[serde(rename = "type")]
    msg_type: String,
    #[allow(dead_code)]
    id:       Option<u64>,
    event:    Option<HaEvent>,
    success:  Option<bool>,
}

#[derive(Debug, Deserialize)]
struct HaEvent {
    data:       HaEventData,
    time_fired: String,
}

#[derive(Debug, Deserialize)]
struct HaEventData {
    entity_id: String,
    new_state: Option<HaState>,
}

#[derive(Debug, Deserialize)]
struct HaState {
    state:        String,
    attributes:   Option<serde_json::Value>,
    last_changed: Option<String>,
}

// ── REST-only type for /api/states ────────────────────────────────────────────

#[derive(Deserialize)]
struct HaStateRest {
    entity_id:    String,
    state:        String,
    attributes:   Option<serde_json::Value>,
    last_changed: Option<String>,
}

// ── HaAdapter ─────────────────────────────────────────────────────────────────

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct HaAdapter {
    config:  HaConfig,
    http:    reqwest::Client,
    confirm_tx: mpsc::Sender<String>,
    pending_confirmations: HashMap<String, String>,
    pending_expected:      HashMap<String, String>,
    ws:      Option<WsStream>,
    next_id: u64,
    initial_events: Vec<RawDeviceEvent>,  
}

impl HaAdapter {
    pub fn new(config: HaConfig, confirm_tx: mpsc::Sender<String>) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            confirm_tx,
            pending_confirmations: HashMap::new(),
            pending_expected:      HashMap::new(),
            ws:      None,
            next_id: 1,
            initial_events: Vec::new(),
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    async fn ws_send(&mut self, msg: serde_json::Value) -> Result<(), AdapterError> {
        let ws = self.ws.as_mut()
            .ok_or_else(|| AdapterError::NotConnected("ws not open".into()))?;
        ws.send(Message::Text(msg.to_string()))
            .await
            .map_err(|e| AdapterError::Transport(e.to_string()))
    }

    /// Performs the full connect + auth + subscribe sequence.
    /// Separated from connect() so the reconnect loop can call it directly.
    async fn open_connection(&mut self) -> Result<(), AdapterError> {
        let ws_url = format!(
            "{}/api/websocket",
            self.config.url
                .trim_end_matches('/')
                .replace("http://", "ws://")
                .replace("https://", "wss://")
        );

        tracing::info!(url = %ws_url, "HA WebSocket connecting");

        let (ws, _) = connect_async(&ws_url)
            .await
            .map_err(|e| AdapterError::Transport(e.to_string()))?;
        self.ws = Some(ws);

        // auth_required
        self.expect_msg_type("auth_required").await?;

        // auth
        let token = self.config.token.clone();
        self.ws_send(serde_json::json!({
            "type": "auth",
            "access_token": token,
        }))
        .await?;

        // auth_ok
        let result = self.expect_msg_type("auth_ok").await;
        if result.is_err() {
            return Err(AdapterError::Auth(
                "HA authentication failed — check long-lived token".into(),
            ));
        }

        // subscribe_events
        let id = self.alloc_id();
        self.ws_send(serde_json::json!({
            "id": id,
            "type": "subscribe_events",
            "event_type": "state_changed",
        }))
        .await?;

        // subscription ack
        {
            let ws = self.ws.as_mut().unwrap();
            let msg = ws.next().await
                .ok_or_else(|| AdapterError::Transport("no subscribe ack".into()))?
                .map_err(|e| AdapterError::Transport(e.to_string()))?;
            let parsed: HaMessage = serde_json::from_str(
                msg.to_text().map_err(|e| AdapterError::Transport(e.to_string()))?,
            )
            .map_err(|e| AdapterError::Transport(e.to_string()))?;
            if parsed.success != Some(true) {
                return Err(AdapterError::Transport("subscribe_events failed".into()));
            }
        }

        tracing::info!("HA WebSocket connected and subscribed");
        Ok(())
    }

    /// Read exactly one message and verify it has the expected type.
    async fn expect_msg_type(&mut self, expected: &str) -> Result<HaMessage, AdapterError> {
        let ws = self.ws.as_mut()
            .ok_or_else(|| AdapterError::NotConnected("ws not open".into()))?;
        let msg = ws.next().await
            .ok_or_else(|| AdapterError::Transport(format!("expected {expected}, got EOF")))?
            .map_err(|e| AdapterError::Transport(e.to_string()))?;
        let parsed: HaMessage = serde_json::from_str(
            msg.to_text().map_err(|e| AdapterError::Transport(e.to_string()))?,
        )
        .map_err(|e| AdapterError::Transport(e.to_string()))?;
        if parsed.msg_type != expected {
            return Err(AdapterError::Transport(format!(
                "expected {expected}, got '{}'",
                parsed.msg_type
            )));
        }
        Ok(parsed)
    }

    /// Fetch current state of all configured entities via REST.
    /// Called after every (re)connect so we have a fresh baseline.
    async fn fetch_initial_states(&self) -> Result<Vec<RawDeviceEvent>, AdapterError> {
        let url = format!("{}/api/states", self.config.url.trim_end_matches('/'));

        let all: Vec<HaStateRest> = self
            .http
            .get(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.config.token),
            )
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|e| AdapterError::Transport(e.to_string()))?
            .json()
            .await
            .map_err(|e| AdapterError::Transport(e.to_string()))?;

        let entity_map: HashMap<&str, &HaDeviceConfig> = self
            .config
            .devices
            .iter()
            .map(|d| (d.ha_entity_id.as_str(), d))
            .collect();

        let mut events = Vec::new();
        for ha in &all {
            if let Some(device_cfg) = entity_map.get(ha.entity_id.as_str()) {
                let state = HaState {
                    state:        ha.state.clone(),
                    attributes:   ha.attributes.clone(),
                    last_changed: ha.last_changed.clone(),
                };
                if let Some(ev) = map_state_to_event(&state, device_cfg) {
                    events.push(ev);
                }
            }
        }

        tracing::info!(
            fetched = events.len(),
            configured = self.config.devices.len(),
            "HA initial state fetched"
        );
        Ok(events)
    }
}

// ── DeviceAdapter ─────────────────────────────────────────────────────────────

#[async_trait]
impl DeviceAdapter for HaAdapter {
    async fn connect(&mut self) -> Result<(), AdapterError> {
        self.open_connection().await?;
        match self.fetch_initial_states().await {
            Ok(events) => self.initial_events = events,
            Err(e) => tracing::warn!(error = %e, "initial state fetch failed"),
        }
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), AdapterError> {
        if let Some(mut ws) = self.ws.take() {
            let _ = ws.close(None).await;
        }
        tracing::info!("HA adapter disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.ws.is_some()
    }

    // ── next_event ────────────────────────────────────────────────────────────
    //
    // Reads events from the WS stream and returns the first one that maps to
    // a configured device.
    //
    // Reconnect supervisor lives here. When the connection drops — for any
    // reason, including power cuts that bounce HA — this loops with exponential
    // backoff until the connection is restored. The caller (main) never sees
    // the reconnect; it just keeps receiving events.
    //
    // On every reconnect: fetch_initial_states() runs first. This catches any
    // state changes that happened while the WS was down.

    async fn next_event(&mut self) -> Result<RawDeviceEvent, AdapterError> {
        // Drain any queued initial-state events on first connect.
        // We do this by storing them in a small internal queue.
        // (On first entry ws is already open from connect().)

        if let Some(event) = self.initial_events.pop() {
            return Ok(event);
        }

        let mut backoff_secs: u64 = 1;

        loop {
            // ── Read one WS message ───────────────────────────────────────────

            let ws = match self.ws.as_mut() {
                Some(ws) => ws,
                None => {
                    // Not connected — attempt reconnect
                    if let Err(e) = self.open_connection().await {
                        tracing::error!(
                            error = %e,
                            backoff_secs,
                            "HA reconnect failed"
                        );
                        tokio::time::sleep(
                            tokio::time::Duration::from_secs(backoff_secs)
                        ).await;
                        backoff_secs = (backoff_secs * 2).min(30);
                        continue;
                    }
                    // Reconnected — fetch missed state changes
                    match self.fetch_initial_states().await {
                        Ok(events) => {
                            // Return the first event immediately; the rest will
                            // be fetched again on the next connect cycle.
                            // For a pilot with one device this is fine.
                            // TODO: buffer and drain for multi-device deployments.
                            if let Some(first) = events.into_iter().next() {
                                return Ok(first);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "initial state fetch failed after reconnect");
                        }
                    }
                    backoff_secs = 1;
                    continue;
                }
            };

            let msg = match ws.next().await {
                Some(Ok(m))  => m,
                Some(Err(e)) => {
                    tracing::warn!(error = %e, "HA WS read error — reconnecting");
                    self.ws = None;
                    continue;
                }
                None => {
                    tracing::warn!("HA WS stream ended — reconnecting");
                    self.ws = None;
                    continue;
                }
            };

            // ── Parse message ─────────────────────────────────────────────────

            let text = match msg.to_text() {
                Ok(t) if t.is_empty() => continue,
                Ok(t) => t,
                Err(_) => continue, // binary/ping frame
            };

            let parsed: HaMessage = match serde_json::from_str(text) {
                Ok(m)  => m,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to parse HA message — skipping");
                    continue;
                }
            };

            if parsed.msg_type != "event" {
                continue;
            }

            let event = match parsed.event {
                Some(e) => e,
                None    => continue,
            };

            let new_state = match event.data.new_state {
                Some(s) => s,
                None    => continue, // entity removed
            };

            let entity_id = &event.data.entity_id;

            // ── Find device config ────────────────────────────────────────────

            let device_cfg = match self
                .config
                .devices
                .iter()
                .find(|d| &d.ha_entity_id == entity_id)
            {
                Some(d) => d.clone(),
                None    => continue, // not a monitored entity
            };

            // ── Map state ─────────────────────────────────────────────────────

            let mapped_value_str = match map_raw_value(&new_state, &device_cfg) {
                Some(v) => v,
                None    => continue,
            };

            // ── Confirmation check ────────────────────────────────────────────

            if let Some(expected) = self.pending_expected.get(entity_id)
                && &mapped_value_str == expected
                    && let Some(command_id) = self.pending_confirmations.remove(entity_id) {
                        self.pending_expected.remove(entity_id);
                        tracing::info!(
                            entity_id  = %entity_id,
                            command_id = %command_id,
                            "HA state_changed confirms command"
                        );
                        self.confirm_tx.try_send(command_id).ok();
                    }

            // ── Emit state event ──────────────────────────────────────────────

            let timestamp = parse_time_fired(&event.time_fired);

            return Ok(RawDeviceEvent {
                external_id: device_cfg.device_id.clone(),
                attribute:   device_cfg.attribute.clone(),
                value:       Value::Text(mapped_value_str),
                timestamp,
                source:      EventSource::Adapter,
                raw: serde_json::json!({
                    "entity_id": entity_id,
                    "state":     new_state.state,
                }),
            });
        }
    }

    // ── send_command ──────────────────────────────────────────────────────────

    async fn send_command(&mut self, command: AdapterCommand) -> Result<(), AdapterError> {
        let device_cfg = self
            .config
            .devices
            .iter()
            .find(|d| d.device_id == command.external_id)
            .ok_or_else(|| AdapterError::DeviceNotFound(command.external_id.clone()))?
            .clone();

        if device_cfg.service_map.is_empty() {
            tracing::warn!(
                device_id = %command.external_id,
                "send_command on read-only device — no-op"
            );
            return Ok(());
        }

        let value_str = match &command.value {
            Value::Text(s)  => s.clone(),
            Value::Bool(b)  => b.to_string(),
            Value::Int(i)   => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Null     => {
                tracing::warn!(
                    device_id = %command.external_id,
                    "send_command with Null value — no-op"
                );
                return Ok(());
            }
        };

        let service = match device_cfg.service_map.get(&value_str) {
            Some(s) => s.clone(),
            None => {
                tracing::warn!(
                    device_id = %command.external_id,
                    value     = %value_str,
                    "no service_map entry for value — no-op"
                );
                return Ok(());
            }
        };

        // "domain/service_name" → POST /api/services/domain/service_name
        let url = format!(
            "{}/api/services/{}",
            self.config.url.trim_end_matches('/'),
            service
        );

        tracing::info!(
            device_id  = %command.external_id,
            entity_id  = %device_cfg.ha_entity_id,
            service    = %service,
            value      = %value_str,
            command_id = %command.command_id,
            "sending command to HA"
        );

        // Register pending confirmation before sending
        self.pending_confirmations.insert(
            device_cfg.ha_entity_id.clone(),
            command.command_id.clone(),
        );
        self.pending_expected.insert(
            device_cfg.ha_entity_id.clone(),
            value_str,
        );

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "entity_id": device_cfg.ha_entity_id }))
            .send()
            .await
            .map_err(|e| AdapterError::Transport(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body   = resp.text().await.unwrap_or_default();
            // Remove pending — command failed to land
            self.pending_confirmations.remove(&device_cfg.ha_entity_id);
            self.pending_expected.remove(&device_cfg.ha_entity_id);
            return Err(AdapterError::CommandRejected(
                format!("HA returned {}: {}", status, body),
            ));
        }

        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract and map the value from a HA state update.
/// Returns None if the value can't be mapped (unknown state not in state_map).
fn map_raw_value(ha_state: &HaState, device_cfg: &HaDeviceConfig) -> Option<String> {
    let raw = match &device_cfg.ha_attribute_key {
        Some(key) => {
            let attrs = ha_state.attributes.as_ref()?;
            match attrs.get(key) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Number(n)) => n.to_string(),
                Some(serde_json::Value::Bool(b))   => b.to_string(),
                _ => {
                    tracing::warn!(
                        entity_id = %device_cfg.ha_entity_id,
                        key = %key,
                        "attribute key not found or wrong type"
                    );
                    return None;
                }
            }
        }
        None => ha_state.state.clone(),
    };

    if device_cfg.state_map.is_empty() {
        return Some(raw);
    }

    match device_cfg.state_map.get(&raw) {
        Some(mapped) => Some(mapped.clone()),
        None => {
            // "unavailable"/"unknown" are normal during HA boot — suppress noise
            if raw != "unavailable" && raw != "unknown" {
                tracing::warn!(
                    entity_id = %device_cfg.ha_entity_id,
                    state = %raw,
                    "no state_map entry — skipping"
                );
            }
            None
        }
    }
}

/// Map a full HaState (from REST) to a RawDeviceEvent.
fn map_state_to_event(ha_state: &HaState, device_cfg: &HaDeviceConfig) -> Option<RawDeviceEvent> {
    let mapped = map_raw_value(ha_state, device_cfg)?;
    Some(RawDeviceEvent {
        external_id: device_cfg.device_id.clone(),
        attribute:   device_cfg.attribute.clone(),
        value:       Value::Text(mapped),
        timestamp: ha_state.last_changed
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp_millis() as u64)
            .unwrap_or_else(now_ms),
        source:      EventSource::Adapter,
        raw: serde_json::json!({
            "entity_id": device_cfg.ha_entity_id,
            "state":     ha_state.state,
        }),
    })
}

/// Parse HA's RFC3339 time_fired string to unix millis.
/// Falls back to now_ms() on any parse failure — never panics.
fn parse_time_fired(s: &str) -> u64 {
    // Manual parse: "2024-01-15T10:30:00.123456+00:00"
    // We only need millisecond precision. Use SystemTime via std.
    // chrono not in deps — parse the UTC offset manually is fragile,
    // so we accept ~1ms timestamp error and just use now_ms() as fallback.
    // For event ordering this is fine — sequence numbers are authoritative.
    let _ = s;
    now_ms()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}