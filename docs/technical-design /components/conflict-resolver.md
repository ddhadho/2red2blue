# Takes a batch of candidate commands, applies priority rules, returns resolved commands.

## Trait and Struct Definitions

```rust
trait ConflictResolver {
    fn resolve(&self, candidates: Vec<Command>) -> ResolvedCommands;
}

struct ResolvedCommands {
    winners: Vec<Command>,
    losers: Vec<ConflictRecord>,
}

struct ConflictRecord {
    command: Command,
    lost_to: RuleId,
    reason: String,
}
```

## Resolution Algorithm

1.  Group commands by (`DeviceId`, `AttributeKey`) — same device same attribute is a conflict.
2.  For each conflict group:
    a.  Sort by rule priority descending.
    b.  Winner is highest priority.
    c.  If tie → most recently modified rule wins.
    d.  All losers recorded in `ConflictRecord`.
3.  Return winners + all loser records.

## Conflict Logging and Visibility

Loser records are written to WAL as `EventKind::RuleConflict`. The UI surfaces these so you can see when rules are fighting and tune priorities. This is operationally important — without visibility into conflicts your rules become a black box.