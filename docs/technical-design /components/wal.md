# Write-Ahead Log (WAL)

The Write-Ahead Log (`WAL`) is a durable, append-only log of all events in the system, crucial for crash recovery and maintaining data consistency.

## Implementation

```rust
pub enum EventPriority {
    Critical,  // command lifecycle — bypass buffer, write immediately
    Normal,    // everything else — buffered, group commit
}

impl EventPriority {
    pub fn for_kind(kind: &EventKind) -> Self {
        match kind {
            EventKind::CommandSent
            | EventKind::CommandConfirmed
            | EventKind::CommandFailed
            | EventKind::SystemBoot
            | EventKind::SystemShutdown => EventPriority::Critical,
            _ => EventPriority::Normal,
        }
    }
}

pub struct Wal {
    pub fn open(config: WalConfig) -> Result<Self, WalError>;
    pub fn append(&mut self, event: Event, priority: EventPriority) -> Result<u64, WalError>;
    pub fn flush(&mut self) -> Result<(), WalError>;
    pub fn replay_from(&self, sequence: u64) -> Result<Vec<Event>, WalError>;
    pub fn latest_sequence(&self) -> u64;
    pub fn latest_snapshot_sequence(&self) -> Result<Option<u64>, WalError>;
    pub fn len(&self) -> u64;
}
```

## Event Tiering

The durability requirement of an event is determined by its **consequence**, not its type. The question is: if this event is lost in a crash, does the system end up in an unsafe state that reconciliation cannot fix?

For most events the answer is no. On boot, reconciliation polls all devices and reconstructs actual state from the physical world. A lost temperature reading, heartbeat, or device state change will be recovered automatically.

The events that must survive a crash are command lifecycle events. If `CommandDispatched` is lost, the system reboots, reconciliation finds the gate unlocked, and cannot distinguish between a failed command and a physical intervention. That ambiguity is unsafe.

**Critical** (`EventPriority::Critical`) — fsynced immediately, bypasses buffer:
- `CommandSent`
- `CommandConfirmed`
- `CommandFailed`
- `SystemBoot`
- `SystemShutdown`

**Normal** (`EventPriority::Normal`) — accumulated in RAM, flushed in batches:
- `DeviceStateChanged`, `RuleTriggered`, `RuleConflict`, and everything else

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
│      write + RESTART checkpoint     │
│                                     │
│  Persistent Storage                 │
│  ├── V1 (eMMC router): SQLite       │
│  │   RESTART checkpoint on critical │
│  │   group commit on normal         │
│  └── V3 (flash router): tmpfs hot   │
│      log + periodic flash snapshot  │
│      critical events write-through  │
└─────────────────────────────────────┘
```

## Group Commit

Normal events accumulate in a RAM buffer and are committed to SQLite in a single transaction
when either threshold is crossed:

| Platform | Storage | Buffer Threshold | Timer |
|---|---|---|---|
| V1 — high-grade router (GL.iNet MT6000 class) | eMMC | 4 KB | 5 seconds |
| V3 — cheap consumer router | SPI flash | 4 KB | 30 seconds |

At typical event rates (one event per ~5 seconds from a single device), the 4 KB buffer takes
minutes to fill, so the timer is the dominant flush trigger in practice.

## Critical Event Durability

Critical events bypass the buffer entirely. Before writing, any pending buffer is flushed.
The event is then written directly to SQLite followed by `PRAGMA wal_checkpoint(RESTART)`.

`RESTART` blocks until all WAL frames are written to the main database file and the OS has
flushed to disk. This is the hard durability guarantee — a power cut after `RESTART` returns
cannot lose the event.

`PASSIVE` checkpoint was considered but rejected — it does not wait for readers and does not
guarantee the write has reached disk. It is not sufficient for critical event durability.

The connection uses `synchronous=NORMAL` so normal buffered writes do not pay the fsync cost.
Only critical events trigger the full `RESTART` checkpoint.

## SQLite Configuration

```sql
PRAGMA journal_mode=WAL;       -- enables concurrent reads during writes
PRAGMA synchronous=NORMAL;     -- normal events: no per-write fsync
PRAGMA cache_size=1000;
PRAGMA temp_store=memory;
```

Critical events override durability at write time via `wal_checkpoint(RESTART)`.

## Replay

```rust
pub fn replay_from(&self, sequence: u64) -> Result<Vec<Event>, WalError>
```

Returns all events from `sequence` onwards in order. Used by the boot sequence after loading
the latest snapshot to replay only the events the snapshot does not cover.

## Snapshots

Snapshots are recorded every `snapshot_interval_events` (default 1000). The snapshot path
and sequence are recorded in the `snapshots` table. The last two snapshots are retained —
older ones are pruned automatically.

`latest_snapshot_sequence()` returns the sequence of the most recent snapshot, used by the
boot sequence to determine the replay start point.

## Boot Sequence

1. Call `latest_snapshot_sequence()` → get sequence N (or 0 if no snapshot exists).
2. Load snapshot state at sequence N.
3. Call `replay_from(N + 1)` → replay all events after the snapshot.
4. Reconcile: poll all devices and resolve any ambiguity from in-flight commands.
5. State is now current.

## Retention Policy

The WAL never deletes event rows. Snapshots act as the compaction mechanism — on boot only
events after the latest snapshot are replayed, so the effective log is always bounded.
The last two snapshots are kept for safety.

## Scope

V1 implements the SQLite backend for the eMMC router target. The V3 tmpfs backend is
deferred — it will implement the same public interface with no daemon changes required.