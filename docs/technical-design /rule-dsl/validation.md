Rules are validated against the device registry on load and hot-reload to ensure their correctness and prevent errors.

## RuleValidator Trait

```rust
struct RuleValidator {
    fn validate(&self, rule: &Rule, devices: &DeviceRegistry)
        -> Result<(), Vec<ValidationError>>;
}
```

## ValidationError Enum

```rust
enum ValidationError {
    UnknownDevice(DeviceId),
    UnknownAttribute(DeviceId, AttributeKey),
    DeviceNotWritable(DeviceId, AttributeKey),
    InvalidOperatorForType(Operator, AttributeType),
    ConflictGroupMissing,
}
```

## Validation Logic

*   **Rules referencing unknown devices are rejected:** Ensures all `device_id`s specified in a rule correspond to existing devices in the system.
*   **Conflict group required for high-priority rules:** High-priority rules must belong to a conflict group to facilitate proper conflict resolution.
*   **Hot-reload applies validation before committing changes:** Validation is performed during hot-reloading to prevent invalid rules from being activated in a running system.