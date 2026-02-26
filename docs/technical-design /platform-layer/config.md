All daemon configuration is managed within a single `config.toml` file. Sensitive information, such as API tokens, are injected securely via environment variables (e.g., `${HA_TOKEN}`).

## Configuration File (config.toml)

```toml
[daemon]
name = "smarthome-daemon"
log_level = "info"          # debug | info | warn | error
log_output = "file"         # file | stdout | syslog

[storage]
wal_path = "/data/smarthome/wal"
snapshot_path = "/data/smarthome/snapshots"
rules_path = "/data/smarthome/rules.toml"
devices_path = "/data/smarthome/devices.toml"
max_wal_size_mb = 512
snapshot_interval_events = 1000

[adapter]
kind = "homeassistant"      # homeassistant | zigbee2mqtt | mock
url = "ws://localhost:8123/api/websocket"
token = "${HA_TOKEN}"       # read from environment, never hardcoded
reconnect_interval_seconds = 5
event_dedup_window_ms = 50

[reconciler]
boot_poll_timeout_seconds = 30
continuous_poll_interval_seconds = 60
confidence_degraded_threshold = 0.5
confidence_unknown_threshold = 0.2

[dispatcher]
command_timeout_seconds = 5
max_retries = 3
retry_delay_ms = 500

[ui]
enabled = true
port = 7000
bind = "0.0.0.0"           # local network only, never exposed to internet

[platform]
kind = "linux"              # linux | openwrt
watchdog_enabled = true
watchdog_interval_seconds = 30
max_memory_mb = 256
```

## Configuration Sections

### `[daemon]`

*   **`name`**: The name of the daemon process.
*   **`log_level`**: Minimum level for logs to be output (`debug`, `info`, `warn`, `error`).
*   **`log_output`**: Where logs are directed (`file`, `stdout`, `syslog`).

### `[storage]`

*   **`wal_path`**: File system path for the Write-Ahead Log.
*   **`snapshot_path`**: Directory for state snapshots.
*   **`rules_path`**: Path to the `rules.toml` file.
*   **`devices_path`**: Path to the `devices.toml` file.
*   **`max_wal_size_mb`**: Maximum size of the WAL before old entries are pruned.
*   **`snapshot_interval_events`**: How many events between snapshots.

### `[adapter]`

*   **`kind`**: Type of home automation platform to connect to (`homeassistant`, `zigbee2mqtt`, `mock`).
*   **`url`**: Connection URL for the adapter.
*   **`token`**: Authentication token (read from environment variables).
*   **`reconnect_interval_seconds`**: Delay between reconnection attempts.
*   **`event_dedup_window_ms`**: Time window for event deduplication.

### `[reconciler]`

*   **`boot_poll_timeout_seconds`**: Timeout for polling devices during boot.
*   **`continuous_poll_interval_seconds`**: Interval for continuous polling of degraded devices.
*   **`confidence_degraded_threshold`**: Confidence level below which a device is considered degraded.
*   **`confidence_unknown_threshold`**: Confidence level below which a device's state is considered unknown.

### `[dispatcher]`

*   **`command_timeout_seconds`**: Timeout for individual command confirmation.
*   **`max_retries`**: Maximum number of retries for failed commands.
*   **`retry_delay_ms`**: Delay between command retry attempts.

### `[ui]`

*   **`enabled`**: Whether the UI server is enabled (`true` or `false`).
*   **`port`**: Port on which the UI server listens.
*   **`bind`**: IP address to bind the UI server to (e.g., `0.0.0.0` for local network access).

### `[platform]`

*   **`kind`**: The operating system or platform (`linux`, `openwrt`).
*   **`watchdog_enabled`**: Whether the watchdog is active.
*   **`watchdog_interval_seconds`**: How often the watchdog expects a heartbeat.
*   **`max_memory_mb`**: Maximum memory usage in megabytes.
