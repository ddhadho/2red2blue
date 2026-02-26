Operators are used within conditions to perform comparisons and checks against subject values.

## Available Operators (V1)

*   `Equals`:           Exact match.
*   `NotEquals`:        Inverse match.
*   `GreaterThan`:      Numeric comparison.
*   `LessThan`:         Numeric comparison.
*   `Between`:          Range check, works for numbers and time.
*   `Changed`:          Detects any change regardless of value.
*   `WasPreviously`:    Checks the previous value before the current change.
*   `IsUnknown`:        Checks if confidence is below a threshold.

## Example: Using `IsUnknown` for Safety Rules

The `IsUnknown` operator allows you to write crucial safety rules.

```toml
[[rules.conditions]]
subject = { device_id = "gate_main", attribute = "state" }
operator = "IsUnknown"
```

This rule can be interpreted as: "If we don't know the gate state — lock it."
