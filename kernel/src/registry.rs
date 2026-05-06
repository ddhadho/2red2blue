use std::collections::HashMap;
use serde::Deserialize;
use tracing::info;
use crate::types::{Device, DeviceId, DeviceKind, Capability, AttributeKey, Value};

#[derive(Debug, Deserialize)]
struct DeviceFile {
    #[serde(default)]
    devices: Vec<DeviceEntry>,
}

#[derive(Debug, Deserialize)]
struct DeviceEntry {
    id: String,
    external_id: String,
    name: String,
    kind: String,
    capabilities: Vec<toml::Value>,
    confidence_decay_seconds: u64,
    safe_default: HashMap<String, toml::Value>,
}

pub struct DeviceRegistry {
    devices: HashMap<DeviceId, Device>,
    external_to_internal: HashMap<String, DeviceId>,
}

impl DeviceRegistry {
    pub fn load(path: &str) -> Result<Self, RegistryError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| RegistryError::IoError(e.to_string()))?;

        let file: DeviceFile = if contents.trim().is_empty() {
            DeviceFile { devices: vec![] }
        } else {
            toml::from_str(&contents)
                .map_err(|e| RegistryError::ParseError(e.to_string()))?
        };

        let mut devices = HashMap::new();
        let mut external_to_internal = HashMap::new();

        for entry in file.devices {
            let device_id = DeviceId(entry.id.clone());

            let kind = match entry.kind.as_str() {
                "Gate"          => DeviceKind::Gate,
                "SecurityLight" => DeviceKind::SecurityLight,
                "BoreholePump"  => DeviceKind::BoreholePump,
                "WaterTank"     => DeviceKind::WaterTank,
                "AlarmPanel"    => DeviceKind::AlarmPanel,
                "Camera"        => DeviceKind::Camera,
                "SmartPlug"     => DeviceKind::SmartPlug,
                "Inverter"      => DeviceKind::Inverter,
                "Generator"     => DeviceKind::Generator,
                "PowerMonitor"  => DeviceKind::PowerMonitor,
                "Sensor"        => DeviceKind::Sensor,
                _               => DeviceKind::Unknown,
            };

            let capabilities = entry.capabilities.iter().map(|c| {
                if let toml::Value::Table(t) = c {
                    if let Some(attr) = t.get("Writable").and_then(|v| v.as_str()) {
                        return Capability::Writable(AttributeKey(attr.to_string()));
                    }
                    if let Some(attr) = t.get("Readable").and_then(|v| v.as_str()) {
                        return Capability::Readable(AttributeKey(attr.to_string()));
                    }
                }
                Capability::Readable(AttributeKey("unknown".to_string()))
            }).collect();

            let safe_default = entry.safe_default.iter()
                .map(|(k, v)| {
                    let value = match v {
                        toml::Value::Boolean(b) => Value::Bool(*b),
                        toml::Value::Integer(i) => Value::Int(*i),
                        toml::Value::Float(f)   => Value::Float(*f),
                        toml::Value::String(s)  => Value::Text(s.clone()),
                        _                       => Value::Null,
                    };
                    (AttributeKey(k.clone()), value)
                })
                .collect();

            let device = Device {
                id: device_id.clone(),
                external_id: entry.external_id.clone(),
                name: entry.name,
                kind,
                capabilities,
                confidence_decay_seconds: entry.confidence_decay_seconds,
                safe_default,
            };

            external_to_internal.insert(entry.external_id, device_id.clone());
            devices.insert(device_id, device);
        }

        info!(count = devices.len(), "device registry loaded");

        Ok(Self { devices, external_to_internal })
    }

    pub fn get(&self, id: &DeviceId) -> Option<&Device> {
        self.devices.get(id)
    }

    pub fn resolve_external(&self, external_id: &str) -> Option<&DeviceId> {
        self.external_to_internal.get(external_id)
    }

    pub fn all(&self) -> impl Iterator<Item = &Device> {
        self.devices.values()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("IO error: {0}")]
    IoError(String),

    #[error("Parse error: {0}")]
    ParseError(String),
}