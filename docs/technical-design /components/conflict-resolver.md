# Conflict Resolver

Takes a batch of candidate commands from the rule engine, applies the priority model, and
returns winners and losers. Stateless — no side effects, no WAL writes, no shared state.
All orchestration happens in main.

## Struct Definition

```rust
pub struct ConflictResolver;

pub struct ResolvedCommands {
    pub winners: Vec<Command>,
    pub losers: Vec<ConflictRecord>,
}

pub struct ConflictRecord {
    pub winner_command: Command,
    pub loser_command: Command,
    pub winner_rule_id: RuleId,
    pub loser_rule_id: RuleId,
    pub device_id: DeviceId,
    pub attribute: AttributeKey,
    pub reason: ConflictReason,
}

pub enum ConflictReason {
    LowerPriority,
    PriorityTie,   // tie broken by rule ordering — loser still recorded
}
```

## Resolution Algorithm

1. Group commands by `(DeviceId, AttributeKey)` — same device, same attribute is a conflict.
2. For each group with more than one command:
   a. Sort by rule priority descending (`Command::rule_id` → look up in priority map).
   b. Winner is highest priority.
   c. Tie → first in sort order wins (deterministic — rule ordering in rules.toml is tiebreak).
   d. All non-winners recorded as `ConflictRecord`.
3. Groups with only one command pass through unconflicted.
4. Return `ResolvedCommands { winners, losers }`.

## Priority Map

The resolver takes a `&HashMap<RuleId, u8>` priority map built by main from the loaded
ruleset. Commands from the rule engine tick (delayed actions, timeouts) carry the originating
`rule_id` so priorities are resolved consistently regardless of when the command was produced.

Commands with no `rule_id` (manual or system commands) are treated as priority 0 — they lose
to any rule-driven command.

## What Main Does With Losers

Main receives `ResolvedCommands`. For each loser:
- Appends a `EventKind::RuleConflict` event to the WAL
- Pushes the `ConflictRecord` into `SharedState::conflicts`

The resolver never touches the WAL or shared state directly.

## Conflict Visibility

Conflicts are surfaced at `GET /conflicts` on the UI server. When two rules fight over the
same device, the operator sees it and can tune priorities. Without this visibility, rules
become a black box.