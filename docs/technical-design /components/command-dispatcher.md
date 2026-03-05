# Command Dispatcher

Takes resolved commands, sends them to the adapter layer, tracks confirmation and failure.
Does not hold references to the state engine, WAL, or shared state — all outcomes are
returned to main for orchestration.

## Struct Definition

```rust
pub struct CommandDispatcher {
    store: CommandStore,           // command_id → Command, owns timeout/retry logic
    pending_send: Vec<Command>,    // commands ready to send — filled by enqueue and tick, drained by main
    failed_commands: Vec<Command>, // permanently failed — filled by tick, drained by main
}
```

`adapter_tx` is **not** held by the dispatcher. It lives in main and is passed into
`send_to_adapter` at the call site. The dispatcher is fully synchronous and has no channel
dependencies — it only tracks state and surfaces outcomes.

Timeout and retry parameters (`max_retries`, `timeout_ms`) are taken from config at
construction time via `CommandDispatcher::new(max_retries, timeout_ms)`.

## Public Interface

```rust
impl CommandDispatcher {
    pub fn new(max_retries: u8, timeout_ms: u64) -> Self;

    // Add command to store and push to pending_send.
    // Main drains pending_send and sends to adapter via send_to_adapter.
    pub fn enqueue(&mut self, command: Command);

    // Called by main after handing command to adapter channel.
    pub fn mark_sent(&mut self, command_id: &str);

    // Route a confirmation event from the ingestor back to the dispatcher.
    // Removes from pending_send (in case confirmation races the tick loop),
    // then removes from store. Returns confirmed command so main can update
    // WAL and shared state.
    pub fn confirm(&mut self, command_id: &str) -> Option<Command>;

    // Drain commands ready to send — main calls after tick to resend retries.
    pub fn drain_pending(&mut self) -> Vec<Command>;

    // Drain permanently failed commands — main calls after tick to surface failures.
    pub fn drain_failed(&mut self) -> Vec<Command>;

    // Called every tick. Checks timeouts, pushes retries to pending_send,
    // pushes failures to failed_commands, then runs store cleanup.
    pub fn tick(&mut self, now_ms: u64);

    pub fn pending_count(&self) -> usize;
    pub fn all_commands(&self) -> Vec<&Command>;
}
```

## Command Flow

1. `enqueue(command)` — inserts into store, pushes to `pending_send`.
2. Main drains `pending_send` and sends each command to adapter via `send_to_adapter`,
   which calls `mark_sent` after a successful channel send.
3. Adapter sends to HA. HA fires a state change event.
4. Ingestor receives the event. Main routes it to `dispatcher.confirm(command_id)`.
5. `confirm` removes from `pending_send` (guards against tick-loop races) and from store.
   Returns the command as `Some(Command)`.
6. Main writes `CommandConfirmed` to WAL, updates shared state.

## Timeout and Retry

`tick` delegates to `CommandStore::tick`, which checks every in-flight command:

- If `now - issued_at < timeout_ms` → still waiting, do nothing
- If `now - issued_at >= timeout_ms` and `retry_count < max_retries` → retry: increment
  `retry_count`, reset `issued_at`, push to `pending_send`, log `Retrying`
- If `retry_count >= max_retries` → remove from store, push to `failed_commands`, log `Failed`

After processing timeouts, `tick` calls `store.cleanup(now_ms)` to prune stale entries.

Main receives failed commands via `drain_failed` and:

- Logs the failure
- Appends a `CommandFailed` event to WAL
- Updates shared state

**Confidence is NOT zeroed on command failure.** Write failure and state freshness are
distinct failure modes. A device can be unreachable for writes while still reporting
accurate state via inbound events. Zeroing confidence on write failure would cause
unnecessary rule re-evaluation and safe-default substitution for a device whose last known
state is still valid. The UI surfaces failed commands separately so the operator can act.

## Confirmation Matching

HA does not send explicit command acknowledgement events — it sends state change events.
The confirmation arrives as a `RawDeviceEvent` with `external_id = "system.command_confirmed"`
and `value = Text(command_id)`. The mock adapter emits this immediately on receipt; a real
HA adapter would emit it when the corresponding state change event arrives from HA.

Main checks `raw_event.external_id` before passing the event to the ingestor. If it matches
`"system.command_confirmed"`, main extracts the `command_id` from `raw_event.value` and
calls `dispatcher.confirm(&command_id)`. The event is then discarded — it does not flow
through the ingestor or state engine.

Matching logic lives in main. Neither the dispatcher nor the ingestor knows about the other.

## Shared State

`CommandDispatcher` does not hold `Arc<Mutex<SharedState>>`. Main updates shared state
after processing each tick's drain results. `SharedState::pending_commands` is a snapshot
of `dispatcher.all_commands()` written by main on each tick and after each event.