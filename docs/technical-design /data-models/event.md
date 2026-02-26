# Event Data Model

Everything that happens in the system is an event: device state changes, system boot, rule triggers, commands sent, commands confirmed — all events. The WAL (Write-Ahead Log) is a sequence of these events.

## Event Struct Definition

```rust
struct Event {
    id: Ulid,                    // sortable, unique, time-ordered
    sequence: u64,               // WAL position, monotonically increasing
    timestamp: SystemTime,       // wall clock
    source: EventSource,         // where it came from
    kind: EventKind,             // what happened
    payload: serde_json::Value,  // flexible, schema per kind
}
```

## EventSource Enum Definition

```rust
enum EventSource {
    Device(DeviceId),
    System,
    Rule(RuleId),
    User,
}
```

## EventKind Enum Definition

```rust
enum EventKind {
    DeviceStateChanged,
    CommandSent,
    CommandConfirmed,
    CommandFailed,
    RuleTriggered,
    RuleConflict,
    SystemBoot,
    SystemShutdown,
    ReconciliationStarted,
    ReconciliationCompleted,
}
```

## ULID vs UUID

ULIDs are time-sortable. You can look at two event IDs and know which came first without querying the sequence number. This is useful for debugging and the UI event log.

## Flexible Payload

Each `EventKind` has a known schema, but encoding it as a JSON value means you can add fields to a payload without requiring a database migration. The sequence number and kind are your stable query surface, not the payload's specific shape.
