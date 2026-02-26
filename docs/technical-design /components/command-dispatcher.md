# Takes resolved commands, sends them to the adapter layer, tracks confirmation.

## Trait Definition

```rust
trait CommandDispatcher {
    async fn dispatch(&mut self, command: Command) -> Result<(), DispatchError>;
    fn pending_commands(&self) -> Vec<&Command>;
    async fn tick(&mut self, now: SystemTime); // checks timeouts, retries
}
```

## Command Flow

1.  Write command to WAL with status `Pending`
2.  Send to HA via REST API
3.  Update WAL status to `Sent`
4.  Wait for confirmation event from HA (via event ingestor)
5.  On confirmation → update WAL status to `Confirmed`
    → update state engine actual state
6.  On timeout (default 5s) → retry up to 3 times
7.  After 3 failures → status `Failed` → emit CommandFailed event
    → state engine marks device confidence `0.0`

## Handling Command Failures

Step 7 is important. A device that consistently fails to confirm commands gets its confidence zeroed. The reconciler then flags it, the UI surfaces it, and if you have a rule like "if gate confidence is zero for 30s → alert homeowner" — that fires automatically.
