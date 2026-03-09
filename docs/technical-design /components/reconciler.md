# Reconciler

Two modes — boot reconciliation runs once before the event loop starts, continuous
reconciliation runs on every tick inside the event loop.

## Structs

```rust
pub struct ReconciliationReport {
    pub completed_at: u64,           // unix millis
    pub devices_evaluated: u32,
    pub mismatches_found: u32,
    pub commands_issued: u32,
    pub skipped_no_actual: u32,      // devices with empty actual on boot
    pub skipped_low_confidence: u32, // devices below degraded threshold
    pub unreachable_devices: Vec<DeviceId>,
}
```

## Boot Reconciliation

Runs once before the event loop starts. Sequence is strict — each step depends on
the previous.

1. Load desired state from snapshot file → `state_engine.set_desired` for each entry
2. Replay WAL from sequence 0 → `state_engine.apply_event` for each `DeviceStateChanged`
   event. Actual state is now as current as the WAL.
3. Start a 30-second boot window. Begin accepting events from the adapter — devices
   report their current state naturally, resetting confidence to 1.0.
4. After the window, call `state_engine.diff()`.
5. For each `StateMismatch`:
   - `actual` is `None` (device never reported) → skip, add to `unreachable_devices`
   - Confidence `< DEGRADED_THRESHOLD` → skip, add to `skipped_low_confidence`
   - Confidence `>= DEGRADED_THRESHOLD` → dispatch correction command
6. Emit `ReconciliationCompleted` event to WAL.
7. Write `ReconciliationReport` to `SharedState::last_reconciliation`.
8. Enter event loop — continuous reconciliation takes over.

The boot window is the key design decision. Devices are not commanded before they have
had a chance to report their actual state. Issuing commands to a device with no known
actual state risks overwriting a state that is already correct.

## Continuous Reconciliation

Runs on every tick in the main event loop. No separate struct — it is additional logic
in the tick branch alongside confidence decay and rule timer evaluation.

```rust
// In tick branch — after state_engine.tick and rule_engine.tick
let mismatches = state_engine.diff();
let correction_commands: Vec<Command> = mismatches
    .into_iter()
    .filter(|m| m.confidence >= degraded_threshold && m.actual.is_some())
    .map(|m| Command::new(
        m.device_id,
        m.attribute,
        m.desired,
        None,       // no rule_id — reconciler-originated
        255,        // highest priority — desired state enforcement
    ))
    .collect();

if !correction_commands.is_empty() {
    dispatch_resolved(correction_commands, ...).await?;
}
```

Continuous reconciliation only acts on devices where actual is known and confidence is
above the degraded threshold. Degraded devices are left alone — the adapter will either
receive new events from them (resetting confidence) or they will decay to unknown and
safe defaults will apply via `get_effective`.

## Mismatch Interpretation

| Condition | Action |
|---|---|
| `actual` is `None` | Never reported — skip, flag as unreachable |
| Confidence `< DEGRADED_THRESHOLD` | Stale actual — skip, wait for device to report |
| Confidence `>= DEGRADED_THRESHOLD` | Trustworthy actual — dispatch correction command |

## Safe Defaults

The reconciler never writes safe defaults into `actual`. Safe defaults are applied at
read time by `DeviceState::get_effective`. When a device is unreachable, the reconciler
leaves `actual` as the last genuine report. The rule engine and any consumer reading
that device receive the safe default via `get_effective` without `actual` being corrupted.

## ReconciliationReport Visibility

`ReconciliationReport` is written to `SharedState::last_reconciliation` after boot
reconciliation completes. The UI serves it at `GET /reconciliation`. The field is
`Option<ReconciliationReport>` — `None` until the first reconciliation completes.
The UI shows "not yet reconciled" for `None`.