# Two modes — boot reconciliation and continuous reconciliation.

## Trait and Struct Definitions

```rust
trait ReconciliationLoop {
    async fn boot_reconcile(&mut self) -> ReconciliationReport;
    async fn continuous_tick(&mut self, degraded: Vec<DeviceId>);
}

struct ReconciliationReport {
    devices_polled: u32,
    conflicts_found: u32,
    commands_issued: u32,
    unknown_devices: Vec<DeviceId>,
}
```

## Boot Reconciliation

1.  WAL replay complete → desired state known.
2.  Poll all devices via adapter layer → actual state known.
3.  For each device:
    a.  Compare desired vs actual.
    b.  If mismatch → generate reconciliation command.
    c.  If device unreachable → apply safe default, mark confidence `0.0`.
4.  Dispatch all reconciliation commands.
5.  Emit `ReconciliationCompleted` event with report.
6.  Begin continuous mode.

## Continuous Reconciliation

Continuous reconciliation handles the `degraded` devices list from `StateEngine::tick`. For each degraded device it issues a poll — a read-only command asking the device to report its current state. If the device responds, confidence resets. If not, confidence continues to decay toward safe default.