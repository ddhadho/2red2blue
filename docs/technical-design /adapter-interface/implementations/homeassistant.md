# Home Assistant Adapter

The HA adapter is the only file in the codebase that knows what a HA WebSocket
message looks like. Everything it receives is normalized before leaving this file.
Everything it sends is translated from the internal format inside this file.

## Implementation
```rust
pub struct HaAdapter {
    config:                  HaConfig,
    confirm_tx:              mpsc::Sender<String>,
    pending_confirmations:   HashMap<String, String>, // ha_entity_id → command_id
    pending_expected_values: HashMap<String, String>, // ha_entity_id → expected value
    ws:                      Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    next_id:                 u64,
    http:                    reqwest::Client,
}
```

## Connection sequence

1. Open WebSocket to `{config.url}/api/websocket` (http → ws, https → wss)
2. Receive `auth_required`
3. Send `{"type":"auth","access_token":"...token..."}`
4. Receive `auth_ok` — fail hard on `auth_invalid`
5. Send `subscribe_events` for `state_changed`
6. Receive `result` with `success: true`
7. Fetch initial state for all configured devices via `GET /api/states/{entity_id}`
   and emit one `RawDeviceEvent` per device — state engine is warm before
   the boot window closes

## Event ingestion — `next_event`

Reads WebSocket frames in a loop. For each frame:

- Empty frames (WebSocket pings) — skip
- Non-text frames — skip
- JSON parse failures — log warning, skip
- `type != "event"` — skip (result messages, subscription confirmations)
- `event.data.entity_id` not in configured devices — skip
- `new_state` is null (entity removed) — skip

For matching events:

1. Extract value from `new_state.state` or `new_state.attributes[ha_attribute_key]`
   when `ha_attribute_key` is set
2. Map through `state_map` if non-empty, skip if no entry found
3. Check `pending_expected_values` — if entity has a pending confirmation and
   the mapped value matches the expected value, fire `confirm_tx.send(command_id)`
4. Return `RawDeviceEvent` with `external_id = ha_entity_id`

## Command dispatch — `send_command`

1. Look up device config by `command.external_id`
2. If `service_map` is empty — log warning, no-op, return `Ok(())`
3. Map value string through `service_map` to get `domain/service`
4. Register `ha_entity_id → command_id` in `pending_confirmations`
5. Register `ha_entity_id → expected_value` in `pending_expected_values`
6. POST to `{url}/api/services/{domain}/{service}` with `{"entity_id": "..."}`
7. On HTTP error — remove pending entries, return `Err(CommandRejected)`

## Command confirmation

Confirmation is state-change watch, not optimistic.

Sending a command registers the `(ha_entity_id, expected_value)` pair. When
`next_event` sees a state change for that entity with the matching value, it
fires `confirm_tx.send(command_id)`. Main routes this to
`dispatcher.confirm(command_id)`.

Confirmation is keyed on `ha_entity_id` only. If a retry arrives before the
first command is confirmed, the new `command_id` overwrites the old one in
`pending_confirmations`. Only the latest command_id is confirmed when the
state change arrives.

The dispatcher owns timeout and retry — if the state change never arrives,
the dispatcher retries after `timeout_ms` and the adapter registers a new
pending confirmation.

## Config

Defined in `[home_assistant]` in `config.toml`. URL comes from `HaConfig.url`.
```toml
[home_assistant]
url   = "http://homeassistant.local:8123"
token = "long-lived-access-token"

[[home_assistant.devices]]
ha_entity_id = "input_boolean.main_gate"
device_id    = "main_gate"
attribute    = "state"
state_map    = { "on" = "unlocked", "off" = "locked" }
service_map  = { "unlocked" = "input_boolean/turn_on", "locked" = "input_boolean/turn_off" }
```

Each device entry maps one HA entity to one internal device. `ha_attribute_key`
is optional — when absent the adapter reads `new_state.state`. When present it
reads `new_state.attributes[key]`.

## Reconnection

The adapter task in `main.rs` wraps the event loop in a reconnect supervisor.
On `AdapterError` from `next_event`, the supervisor waits 5 seconds and calls
`connect()` again. The WebSocket subscription is re-established on every
reconnect. Pending confirmations are not cleared on reconnect — commands in
flight before a disconnect will be retried by the dispatcher and re-registered
when resent.

## What this adapter does not do

- No WebSocket command dispatch — all commands go via REST (`/api/services`)
- No `list_devices` — device mapping is explicit in config, not discovered
- No device discovery — explicit config is intentional. Automatic discovery
  requires trust in HA's entity model that V1 does not have. Installer maps
  entities deliberately. Discovery is a post-pilot UI feature.
- No attribute polling — state comes from the event stream only, except for
  the initial state fetch on connect
- No TLS certificate validation override — uses system trust store