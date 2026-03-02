# Rule Engine

Stateless evaluator per rule firing. Given the current world model, evaluates all enabled
rules against a `StateUpdate` and produces candidate commands. Never sends commands directly —
all output goes to the conflict resolver first.

## Struct Definition

```rust
pub struct RuleEngine {
    rules: Vec<Rule>,
    rule_states: HashMap<RuleId, RuleState>,
    unknown_threshold: f32,          // from ReconcilerConfig
}

struct RuleState {
    rule_id: RuleId,
    condition_met_at: Option<SystemTime>,  // for duration conditions
    in_flight: Vec<InFlightEntry>,
}

struct InFlightEntry {
    execute_at: SystemTime,
    actions: Vec<Action>,
    kind: InFlightKind,
}

enum InFlightKind {
    DelayedAction,
    Timeout,
}
```

## Public Interface

```rust
impl RuleEngine {
    pub fn new(unknown_threshold: f32) -> Self;

    // Load rules from TOML — validates against device registry
    pub fn load_rules(
        &mut self,
        rules: Vec<Rule>,
        registry: &DeviceRegistry,
    ) -> Result<(), RuleLoadError>;

    // Called by main loop on every StateUpdate
    pub fn evaluate(
        &mut self,
        update: &StateUpdate,
        state: &HashMap<DeviceId, DeviceState>,
        now: SystemTime,
    ) -> Vec<Command>;

    // Called by main loop on every tick — fires expired in-flight entries
    pub fn tick(
        &mut self,
        now: SystemTime,
    ) -> Vec<Command>;

    // Called on SIGUSR1 — hot-reloads rules, preserves in-flight for surviving rule IDs
    pub fn hot_reload(
        &mut self,
        rules: Vec<Rule>,
        registry: &DeviceRegistry,
        now: SystemTime,
    ) -> Result<HotReloadReport, RuleLoadError>;
}
```

## Evaluation Flow

For each enabled rule on every `evaluate` call:

1. **Check trigger** — does `StateUpdate::changed_devices` contain the rule's trigger device
   and attribute? If no → skip this rule entirely.

2. **Evaluate conditions** — for each condition in order:
   - Resolve subject via `get_effective` (or `previous` for `WasPreviously`, or confidence
     check for `IsUnknown`)
   - If `get_effective` returns `None` for any non-`IsUnknown` condition → rule is
     unevaluable → skip
   - Apply operator — if false → skip rule
   - If condition has `duration` → check `condition_met_at`:
     - Not set → record `now`, skip rule (start the clock)
     - Set → if `now - condition_met_at < duration` → skip (not elapsed yet)
     - Set → if elapsed → proceed

3. **All conditions passed** — produce commands for each action:
   - Action with no `delay` → immediate command, add to output
   - Action with `delay` → create `InFlightEntry` with `execute_at = now + delay`,
     write to WAL, add to `rule_states`

4. **StatefulConfig** — if rule has `stateful`:
   - If existing timeout entry → replace with new one (timer reset)
   - If no existing entry → create timeout entry, write to WAL

5. **Reset `condition_met_at`** — clear it after the rule fires so duration
   tracking starts fresh next time.

## Operator Evaluation

| Operator | Reads from | Behaviour on None |
|---|---|---|
| `Equals` | `get_effective` | Unevaluable — skip rule |
| `NotEquals` | `get_effective` | Unevaluable — skip rule |
| `GreaterThan` | `get_effective` | Unevaluable — skip rule |
| `LessThan` | `get_effective` | Unevaluable — skip rule |
| `Between` | `get_effective` | Unevaluable — skip rule |
| `Changed` | `StateUpdate` | N/A — no value read |
| `WasPreviously` | `DeviceState::previous` | False — condition fails |
| `IsUnknown` | confidence value | N/A — always evaluable |

## Tick — In-Flight Expiry

`tick` is called every second by the main loop alongside `StateEngine::tick`. It walks all
in-flight entries across all rule states and fires any whose `execute_at <= now`:

```
for each rule_state:
    for each in_flight entry:
        if now >= entry.execute_at:
            dispatch entry.actions as commands
            remove entry
```

Fired entries are removed from the in-flight map and their WAL records are marked complete.

## Rule Validation on Load

When `load_rules` or `hot_reload` is called, every rule is validated against the device
registry before being accepted:

- Trigger `device_id` must exist in registry
- Every condition `device_id` must exist in registry
- Every action `device_id` must exist in registry
- Every referenced `attribute` must be in the device's `capabilities`

Rules that fail validation are rejected with a `RuleLoadError` that names the offending
rule ID and the missing device or attribute. The daemon logs the error and continues with
the valid rules — it does not crash on a bad rule file.

## Hot-Reload — SIGUSR1

On `SIGUSR1`, the daemon reloads `rules.toml` and calls `hot_reload`:

1. Validate new ruleset against registry — reject invalid rules, log errors
2. For each rule ID in the new ruleset that also exists in the current ruleset:
   - Keep existing `RuleState` including all in-flight entries unchanged
3. For each rule ID in the current ruleset not present in the new ruleset:
   - Cancel all in-flight entries — log each cancellation with rule ID and action
   - Remove `RuleState`
4. For each rule ID in the new ruleset not present in the current ruleset:
   - Create fresh `RuleState` — no in-flight, no `condition_met_at`
5. Replace `self.rules` with the new ruleset

```rust
struct HotReloadReport {
    rules_loaded: u32,
    rules_rejected: u32,
    rules_added: u32,
    rules_removed: u32,
    in_flight_cancelled: u32,
}
```

The reload is logged at info level with the full report. Any in-flight cancellations are
logged at warn level.

## Command Production Principle

The rule engine never sends commands. It returns `Vec<Command>` to the caller. The main loop
passes these to the conflict resolver before anything reaches the dispatcher. This boundary
makes conflict resolution possible — the resolver sees all candidate commands from all rules
before any are sent.