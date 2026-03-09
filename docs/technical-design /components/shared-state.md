# Shared State

`SharedState` is the single data structure shared between the main event loop and the UI
server. Main holds it behind `Arc<Mutex<SharedState>>` and updates it after every event,
tick, and dispatch outcome. The UI server reads it to serve HTTP responses.

## Struct Definition

```rust
pub struct SharedState {
    // Current device state — updated after every event and every tick
    pub devices: HashMap<DeviceId, DeviceState>,

    // Conflict records — appended when resolver produces losers
    // Capped at last 100 entries — oldest pruned first
    pub conflicts: Vec<ConflictRecord>,

    // Commands currently in-flight — snapshot of dispatcher pending map
    // Updated on each tick
    pub pending_commands: Vec<Command>,

    // Report from the most recent boot reconciliation.
    // None until the first reconciliation completes.
    // The UI shows "not yet reconciled" for None.
    pub last_reconciliation: Option<ReconciliationReport>,
}
```

## Update Points

Main updates shared state at these points:

| Event | What changes |
|---|---|
| Device state changed | `devices` |
| Tick (confidence decay) | `devices` |
| Rule conflict | `conflicts` — append loser record |
| Command dispatched | `pending_commands` |
| Command confirmed | `pending_commands` — remove entry |
| Command failed | `pending_commands` — remove entry, `devices` via zero_confidence |
| Boot reconciliation completed | `last_reconciliation` |

## UI Endpoints

Four endpoints: `/state`, `/conflicts`, `/commands`, `/reconciliation`.

### `GET /state`
Returns `SharedState::devices` as JSON.

### `GET /conflicts`
Returns `SharedState::conflicts` as JSON — most recent first.
Capped at last 100 records. Older conflicts are pruned when the cap is reached.

### `GET /commands`
Returns `SharedState::pending_commands` as JSON.
Shows commands currently awaiting confirmation from the adapter.

### `GET /reconciliation`
Returns `SharedState::last_reconciliation` as JSON.
Returns `{"status":"not_yet_reconciled"}` if `last_reconciliation` is `None`.
Shows the report from the most recent boot reconciliation — devices evaluated,
mismatches found, commands issued, and any unreachable devices.