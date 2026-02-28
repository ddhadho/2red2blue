# Reconciliation Loop

Two modes — boot reconciliation and continuous reconciliation.

## Trait and Struct Definitions

```rust
trait ReconciliationLoop {
    async fn boot_reconcile(&mut self) -> ReconciliationReport;
    async fn continuous_tick(&mut self, degraded: Vec<DeviceId>);
}

struct ReconciliationReport {
    devices_polled: u32,
    mismatches_found: u32,
    commands_issued: u32,
    unreachable_devices: Vec<DeviceId>,
}
```

## Boot Reconciliation

1. WAL replay complete → desired state known.
2. Poll all devices via adapter layer → actual state known.
3. For each device:
   - If device responds → `apply_event` updates `actual`, confidence resets to `1.0`.
   - If device unreachable → mark confidence `0.0`, leave `actual` untouched.
4. Call `state_engine.diff()` → get all `StateMismatch` entries.
5. For each mismatch:
   - If confidence `>= DEGRADED_THRESHOLD` → dispatch correction command.
   - If confidence `< DEGRADED_THRESHOLD` → skip, continuous reconciliation will poll.
6. Emit `ReconciliationCompleted` event with report.
7. Begin continuous mode.

## Continuous Reconciliation

Continuous reconciliation has two responsibilities driven by different signals.

**Polling degraded devices** — driven by `StateEngine::tick`. For each device in the degraded
list, issue a poll via the adapter. If the device responds, the ingestor processes the response
as a normal event and confidence resets. If the device does not respond, confidence continues
to decay. `get_effective` handles the safe default at read time — the reconciler does not write
anything into `actual`.

**Correcting confirmed mismatches** — driven by `state_engine.diff()`. For each `StateMismatch`
where confidence is high enough to trust `actual`, dispatch a correction command to bring the
device in line with desired state.

These two responsibilities are intentionally separate. Polling is about recovering information.
Correcting is about enforcing intent. A device should be polled before it is corrected.

## Mismatch Interpretation

The reconciler interprets each `StateMismatch` from `diff()` as follows:

| Condition | Action |
|---|---|
| `actual` is `None` | Device never reported — poll only, no command |
| Confidence `< DEGRADED_THRESHOLD` | Actual is stale — poll first, re-evaluate after |
| Confidence `>= DEGRADED_THRESHOLD` | Actual is trustworthy — dispatch correction command |

## Safe Defaults

The reconciler never writes safe defaults into `actual`. Safe defaults are applied at read
time by `DeviceState::get_effective`. When a device is unreachable the reconciler marks
confidence `0.0` and leaves `actual` as the last genuine report. The rule engine and any
other consumer reading that device will receive the safe default via `get_effective` without
`actual` being corrupted.