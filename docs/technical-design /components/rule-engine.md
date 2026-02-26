# Stateless evaluator. Given current world model, evaluate all enabled rules and produce candidate commands.

## Trait and Struct Definitions

```rust
trait RuleEngine {
    async fn evaluate(&mut self, update: StateUpdate,
                state: &HashMap<DeviceId, DeviceState>) -> Vec<Command>;
    fn load_rules(&mut self, rules: Vec<Rule>);
    fn add_rule(&mut self, rule: Rule);
    fn disable_rule(&mut self, id: RuleId);
}
```

Internally it maintains a `RuleState` per stateful rule — tracking when conditions first became true, managing in-flight timers.

```rust
struct RuleState {
    rule_id: RuleId,
    condition_met_at: Option<SystemTime>,   // for duration conditions
    in_flight: Option<InFlightState>,       // for stateful rules
}

struct InFlightState {
    triggered_at: SystemTime,
    timeout_at: SystemTime,
    pending_actions: Vec<Action>,
}
```

## Evaluation Flow per Rule

1.  Check trigger — does this `StateUpdate` affect this rule's trigger device? If no → skip.
2.  Evaluate conditions against current state.
    For duration conditions → check `condition_met_at`, if not set → record now and skip.
    If set → check if duration elapsed → if yes → proceed.
3.  Produce candidate commands for each action.
4.  Record rule as triggered in WAL.

## Command Production Principle

The rule engine never sends commands. It only produces them. This is the boundary that makes conflict resolution possible.
