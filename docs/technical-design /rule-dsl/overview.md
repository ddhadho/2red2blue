The Smarthome Daemon uses a declarative Rule DSL to automate devices.

## Rule Components

Each rule consists of:

1.  **Trigger** — an event that initiates the rule.
2.  **Conditions** — boolean tests that must all pass.
3.  **Actions** — operations executed when conditions pass.
4.  **Stateful behaviors** — optional timeouts or delayed actions.

This DSL ensures deterministic, WAL-safe automation with conflict resolution and safety operators.

## Design Constraints

The DSL needs to satisfy three things simultaneously:

1.  It must be expressive enough to handle real automations — duration conditions, time of day, multi-device conditions, stateful in-flight logic.
2.  It must be simple enough that a non-developer can write rules through the UI without writing code.
3.  It must be serializable — rules are stored in your state store and reconstructed on boot, so they can't be arbitrary code.

This rules out embedding a general scripting language for rule definitions.

## How Rules Are Stored and Loaded

Rules live in a `rules.toml` file in the config directory. On boot, the rule engine loads all rules, validates them against the device registry, and rejects any rule that references a `device_id` that doesn't exist. This prevents silent failures where a rule does nothing because a `device_id` has a typo.

Rules are hot-reloaded at runtime without restarting the daemon.

The UI can add and modify rules via the `POST /rules` endpoint. Changes are written back to `rules.toml` immediately and hot-reloaded into the rule engine without a daemon restart.

## V1 Limitations

Some things are deliberately out of scope for V1:

*   **Cross-device arithmetic** — "if tank A plus tank B combined level is below 40%." Too complex for V1.
*   **Probabilistic conditions** — "if motion is detected more than 3 times in 10 minutes." Possible but needs a counter primitive not in the current model.
*   **External triggers** — "if it rains" via a weather API. Zero cloud, so no.

These are V2 features. The DSL is designed to be extended — adding a new `Operator` or a new `Trigger` kind doesn't break existing rules.