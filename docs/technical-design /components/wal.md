# Write-Ahead Log (WAL)

The Write-Ahead Log (`WAL`) is a durable, append-only log of all events in the system, crucial for crash recovery and maintaining data consistency.

## Trait Definition

```rust
trait WAL {
    fn append(&mut self, event: Event) -> Result<Sequence, WALError>;
    fn read_from(&self, sequence: Sequence) -> impl Iterator<Item = Event>;
    fn latest_sequence(&self) -> Sequence;
    fn snapshot(&mut self) -> Result<Snapshot, WALError>;
}
```

## Internal Implementation

Internally, it's an append-only file on disk. Every append is an `fsync` — you pay the write latency cost but you never lose an event. On SQLite in WAL mode, this is handled for you.

## Snapshots

Snapshots are periodic — every N events or every T minutes, the current full state is written to a snapshot file. On boot, you load the latest snapshot then replay only the WAL entries after the snapshot sequence. This keeps boot time fast as the WAL grows.

## Boot Sequence

1.  Find latest snapshot → load state at sequence N.
2.  Replay WAL from sequence N+1 → present.
3.  State is now current as of last event before crash.

## Retention Policy

The WAL never deletes entries until a newer snapshot covers them. The last two snapshots are kept for safety.