Conditions are boolean checks that are evaluated when a rule's trigger fires. All conditions within a rule must pass for the rule to proceed (`AND` semantics).

## TOML Example

```toml
[[rules.conditions]]
subject = { device_id = "gate_main", attribute = "state" }
operator = "Equals"
value = "open"

[[rules.conditions]]
subject = { kind = "TimeOfDay" }
operator = "Between"
value = ["22:00", "06:00"]
```

## Structure of a Condition

Each condition typically consists of:

*   **`subject`**: Specifies what is being evaluated (e.g., a device attribute or a time-of-day).
*   **`operator`**: The comparison operator to use (e.g., `Equals`, `Between`, `GreaterThan`).
*   **`value`**: The value(s) to compare against.
