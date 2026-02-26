# Write-Ahead Log (WAL)

The Write-Ahead Log (`WAL`) is a durable, append-only log of all events in the system, crucial for crash recovery and maintaining data consistency.

## Trait Definition

```rust
pub enum EventPriority {
    Critical,   // command lifecycle — bypasses buffer, fsynced immediately
    Normal,     // everything else — buffered and flushed in batches
}

pub trait Wal {
    fn append(&mut self, event: Event, priority: EventPriority) -> Result<Sequence, WalError>;
    fn flush(&mut self) -> Result<(), WalError>;
    fn replay_from(&self, sequence: Sequence) -> impl Iterator<Item = Event>;
    fn latest_sequence(&self) -> Sequence;
    fn snapshot(&mut self) -> Result<Snapshot, WalError>;
}
```

## Event Tiering

The durability requirement of an event is determined by its **consequence**, not its type. The question is: if this event is lost in a crash, does the system end up in an unsafe state that reconciliation cannot fix?

For most events the answer is no. On boot, reconciliation polls all devices and reconstructs actual state from the physical world. A lost temperature reading, heartbeat, or device state change will be recovered automatically.

The events that must survive a crash are command lifecycle events. If `CommandDispatched` is lost, the system reboots, reconciliation finds the gate unlocked, and cannot distinguish between a failed command and a physical intervention. That ambiguity is unsafe.

**Critical** (`EventPriority::Critical`) — fsynced immediately, bypasses buffer:
- `CommandDispatched`
- `CommandConfirmed`
- `CommandFailed`

**Normal** (`EventPriority::Normal`) — accumulated in RAM, flushed in batches:
- `DeviceStateChanged`, `RuleTriggered`, `RuleConflict`, system events, everything else

## Architecture

```
┌─────────────────────────────────────┐
│           WAL Manager               │
│                                     │
│  Hot Buffer (RAM)                   │
│  ├── Normal events accumulate here  │
│  └── Flushed on: timer / size       │
│                                     │
│  Critical Bypass                    │
│  └── Command events → immediate     │
│      write to persistent storage    │
│                                     │
│  Persistent Storage                 │
│  ├── V1 (eMMC router): SQLite       │
│  │   fsync on critical, group       │
│  │   commit on normal               │
│  └── V3 (flash router): tmpfs hot   │
│      log + periodic flash snapshot  │
│      critical events write-through  │
└─────────────────────────────────────┘
```

The `Wal` trait is platform-agnostic. The storage backend and flush thresholds are injected at startup via platform configuration. Daemon code does not change between V1 and V3 deployments.

## Group Commit

Normal events are not fsynced individually. They accumulate in a RAM buffer and are committed to storage as a batch — flushed when the buffer reaches a size threshold or a timer fires. This is group commit, the mechanism used by PostgreSQL, SQLite in WAL mode, and most production storage systems.

Platform-aware thresholds:

| Platform | Storage | Buffer Threshold | Timer | Critical Events |
|---|---|---|---|---|
| V1 — high-grade router (GL.iNet MT6000 class) | eMMC | 4 KB | 5 seconds | Bypass buffer, fsync immediately |
| V3 — cheap consumer router | SPI flash | 4 KB | 30 seconds | Bypass buffer, write-through to flash |

At typical event rates (one event per ~5 seconds from a single device), the 4 KB buffer takes minutes to fill, so the timer is the dominant flush trigger in practice.

## Storage Backends

### V1 — eMMC Router (SQLite)

SQLite in WAL mode with group commit. Normal events accumulate in the RAM buffer and are written in a single transaction on flush. Critical events bypass the buffer and are written and fsynced immediately.

eMMC has built-in controller-level wear leveling, which spreads writes across cells transparently. Rated at 3,000–10,000 write cycles per cell, but effective lifespan is much longer due to leveling. Aggressive fsync on every normal event would be survivable on eMMC, but group commit is built in from the start because it costs one afternoon and makes V3 a configuration change rather than a rewrite.

### V3 — Cheap Flash Router (tmpfs + Snapshot)

SPI flash has no wear leveling. Raw endurance is high per-cell but writes concentrate on the same sectors, making frequent fsync a device-bricking risk in practice.

The hot WAL lives in tmpfs (RAM). Periodic snapshots flush accumulated state to flash at controlled intervals. Critical events write through to flash immediately, bypassing the snapshot timer.

The gap between the last snapshot and a crash is the durability window for normal events. This is acceptable because reconciliation recovers them on reboot. The write-through path for critical events preserves the command-lifecycle safety guarantee.

This backend shares the same `Wal` trait as V1. No daemon changes required.

## Snapshots

Snapshots are periodic — every N events or every T minutes, the current full state is written to a snapshot file. On boot, the latest snapshot is loaded and only WAL entries after the snapshot sequence are replayed. This keeps boot time fast as the WAL grows.

## Boot Sequence

1. Find latest snapshot → load state at sequence N.
2. Replay WAL from sequence N+1 → present.
3. Reconcile: poll all devices and resolve any ambiguity from in-flight commands found in the log.
4. State is now current.

## Retention Policy

The WAL never deletes entries until a newer snapshot covers them. The last two snapshots are kept for safety.

## Scope

Implement the SQLite backend for the V1 eMMC router target, with group commit for normal events and critical bypass for command lifecycle events. The `EventPriority` parameter and `flush` method are part of the interface from day one so the V3 flash backend can be added later as a new implementation with no interface or daemon changes.