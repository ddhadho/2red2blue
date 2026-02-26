# Command Data Model

The `Command` data model represents an instruction to a device, encapsulating its details and lifecycle.

## Struct and Enum Definitions

```rust
struct Command {
    id: Ulid,
    rule_id: Option<RuleId>,     // which rule produced this, if any
    device_id: DeviceId,
    attribute: AttributeKey,
    value: AttributeValue,
    issued_at: SystemTime,
    status: CommandStatus,
    retry_count: u8,
}

enum CommandStatus {
    Pending,
    Sent,
    Confirmed,
    Failed(String),
    Timeout,
}
```

## Command Tracking

Every command is tracked. When a command is sent, it's written to WAL. When confirmed, that confirmation is written to WAL. If the daemon restarts and sees a `Sent` command with no confirmation, it knows to retry or flag it. This is how you detect the "command sent but device didn't respond" failure mode.