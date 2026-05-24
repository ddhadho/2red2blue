use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub daemon:         DaemonConfig,
    pub storage:        StorageConfig,
    pub adapter:        AdapterConfig,
    pub reconciler:     ReconcilerConfig,
    pub dispatcher:     DispatcherConfig,
    pub ui:             UiConfig,
    pub platform:       PlatformConfig,
    pub home_assistant: Option<HaConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DaemonConfig {
    pub name:       String,
    pub log_level:  String,
    pub log_output: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub wal_path:                 String,
    pub snapshot_path:            String,
    pub rules_path:               String,
    pub devices_path:             String,
    pub desired_state_path:       String,
    pub max_wal_size_mb:          u64,
    pub snapshot_interval_events: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AdapterConfig {
    pub kind:                       String,
    pub url:                        String,
    pub reconnect_interval_seconds: u64,
    pub event_dedup_window_ms:      u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReconcilerConfig {
    pub boot_window_secs:                 u64,
    pub continuous_poll_interval_seconds: u64,
    pub confidence_degraded_threshold:    f32,
    pub confidence_unknown_threshold:     f32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DispatcherConfig {
    pub timeout_ms:     u64,
    pub max_retries:    u8,
    pub retry_delay_ms: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UiConfig {
    pub enabled: bool,
    pub port:    u16,
    pub bind:    String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PlatformConfig {
    pub kind:                      String,
    pub watchdog_enabled:          bool,
    pub watchdog_interval_seconds: u64,
    pub max_memory_mb:             u64,
}

// ── Home Assistant ────────────────────────────────────────────────────────────
//
// Lives in kernel::config so adapters can import from kernel without a
// circular dependency. kernel does not depend on adapters.

#[derive(Debug, Deserialize, Clone)]
pub struct HaConfig {
    pub url:     String,
    pub token:   String,
    #[serde(default)]
    pub devices: Vec<HaDeviceConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HaDeviceConfig {
    pub ha_entity_id:     String,
    pub device_id:        String,
    pub attribute:        String,
    pub ha_attribute_key: Option<String>,
    #[serde(default)]
    pub state_map:        HashMap<String, String>,
    #[serde(default)]
    pub service_map:      HashMap<String, String>,
}

// ── Load + validate ───────────────────────────────────────────────────────────

impl Config {
    pub fn load(path: &str) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadFailed(path.to_string(), e.to_string()))?;

        let config: Config = toml::from_str(&contents)
            .map_err(|e| ConfigError::ParseFailed(e.to_string()))?;

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let valid_log_levels = ["debug", "info", "warn", "error"];
        if !valid_log_levels.contains(&self.daemon.log_level.as_str()) {
            return Err(ConfigError::InvalidValue(
                "daemon.log_level".to_string(),
                self.daemon.log_level.clone(),
            ));
        }

        let valid_adapters = ["homeassistant", "zigbee2mqtt", "mock"];
        if !valid_adapters.contains(&self.adapter.kind.as_str()) {
            return Err(ConfigError::InvalidValue(
                "adapter.kind".to_string(),
                self.adapter.kind.clone(),
            ));
        }

        let valid_platforms = ["linux", "openwrt"];
        if !valid_platforms.contains(&self.platform.kind.as_str()) {
            return Err(ConfigError::InvalidValue(
                "platform.kind".to_string(),
                self.platform.kind.clone(),
            ));
        }

        if self.ui.port < 1024 {
            return Err(ConfigError::InvalidValue(
                "ui.port".to_string(),
                self.ui.port.to_string(),
            ));
        }

        if self.adapter.kind == "homeassistant" && self.home_assistant.is_none() {
            return Err(ConfigError::InvalidValue(
                "home_assistant".to_string(),
                "required when adapter.kind = homeassistant".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file '{0}': {1}")]
    ReadFailed(String, String),

    #[error("Failed to parse config: {0}")]
    ParseFailed(String),

    #[error("Invalid value for '{0}': '{1}'")]
    InvalidValue(String, String),
}