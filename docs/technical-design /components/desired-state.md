# Desired State Snapshot

Desired state is persisted to a separate JSON file, independent of the WAL. The WAL
records actual state observations. The desired state snapshot records intent. They solve
different problems and have different persistence requirements.

## Why Not WAL Replay

Deriving desired state from WAL replay requires tracking `CommandSent` and
`CommandConfirmed` pairs, handling orphaned confirmations from pruned WAL segments, and
re-running confirmation tracking logic against historical events. Edge cases accumulate.

The snapshot approach is simpler and more robust. Desired state is small — one value
per device attribute — and changes infrequently. A flat file is sufficient.

## Snapshot Format

JSON file at the path configured in `config.toml` under `storage.desired_state_path`.

```json
{
  "version": 1,
  "written_at": 1741200000000,
  "devices": {
    "gate_main": {
      "state": "locked"
    },
    "security_lights_front": {
      "state": "on"
    },
    "borehole_pump": {
      "state": "off"
    }
  }
}
```

## Write Timing

Desired state is **not** written on every `set_desired` call. Writing on every call
is unnecessary on a system where commands are infrequent and harmful on flash storage
where every write costs a write cycle.

Write schedule:
- On clean shutdown (`SIGINT` / `SIGTERM`) — always
- Periodically every 30 seconds if the snapshot is dirty — covers ungraceful shutdown

The snapshot is marked dirty on every `set_desired` call and clean after each flush.
Losing the last few seconds of desired state changes on a hard crash is acceptable —
the boot window gives devices time to report their actual state, and the reconciler
re-evaluates from there.

## DesiredStateStore

```rust
pub struct DesiredStateStore {
    path: String,
    state: HashMap<DeviceId, HashMap<AttributeKey, Value>>,
    dirty: bool,
    last_flush: SystemTime,
    flush_interval: Duration,  // default 30s
}

impl DesiredStateStore {
    pub fn load(path: &str) -> Result<Self, StoreError>;
    pub fn set(&mut self, device_id: &DeviceId, attr: AttributeKey, value: Value);
    pub fn get_all(&self) -> &HashMap<DeviceId, HashMap<AttributeKey, Value>>;
    pub fn flush_if_needed(&mut self, now: SystemTime) -> Result<(), StoreError>;
    pub fn flush(&mut self) -> Result<(), StoreError>;  // forced — shutdown path
}
```

`flush_if_needed` is called in the tick branch alongside WAL buffer flushing.
`flush` is called explicitly on shutdown before `wal.flush()`.

## Boot Load Path

```
config.storage.desired_state_path
    → DesiredStateStore::load()
        → for each (device_id, attr, value): state_engine.set_desired()
    → WAL::replay_from(0)
        → for each DeviceStateChanged: state_engine.apply_event()
    → 30-second boot window
    → state_engine.diff()
    → reconciler issues correction commands
```

If the snapshot file does not exist (first boot), `DesiredStateStore::load` returns an
empty store. No desired state is set. The reconciler finds no mismatches and the
`ReconciliationReport` reflects zero commands issued. Devices report their actual state
through the normal event flow.

## Relationship to WAL

| | WAL | Desired State Snapshot |
|---|---|---|
| Records | Actual state observations | Intent |
| Written | On every event | On flush (periodic + shutdown) |
| Read | On boot for replay | On boot for reconciliation |
| Pruned | Eventually (snapshots) | Overwritten on each flush |
| Loss on hard crash | Last buffer window (5–30s) | Last 30s of desired changes |