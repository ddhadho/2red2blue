use tracing::{error, info};
use crate::types::Command;
use crate::command_store::{CommandStore, TimeoutAction};

pub struct CommandDispatcher {
    store: CommandStore,
    // Commands ready to send to adapter — filled by enqueue and tick, drained by main
    pending_send: Vec<Command>,
    // Commands that permanently failed — filled by tick, drained by main.
    // Main logs them and updates shared state. Confidence is NOT zeroed —
    // write failure and state freshness are distinct failure modes.
    failed_commands: Vec<Command>,
}

impl CommandDispatcher {
    pub fn new(max_retries: u8, timeout_ms: u64) -> Self {
        Self {
            store: CommandStore::new(max_retries, timeout_ms),
            pending_send: Vec::new(),
            failed_commands: Vec::new(),
        }
    }

    // ── Enqueue from conflict resolver ───────────────────────
    //
    // Delayed actions are handled by rule_engine.tick before reaching
    // the resolver — every command arriving here is immediate.
    pub fn enqueue(&mut self, command: Command) {
        info!(
            command_id = %command.id,
            device_id = %command.device_id,
            attribute = %command.attribute,
            value = %command.value,
            "command enqueued for dispatch"
        );
        self.store.insert(command.clone());
        self.pending_send.push(command);
    }

    // ── Mark sent — called by main after handing to adapter ──
    pub fn mark_sent(&mut self, command_id: &str) {
        self.store.mark_sent(command_id);
    }

    // ── Confirm — called by main when ingestor sees CommandConfirmed ──
    //
    // Removes from pending_send in case confirmation races the tick loop,
    // then removes from the store. Returns the confirmed command so main
    // can write to WAL and update shared state.
    pub fn confirm(&mut self, command_id: &str) -> Option<Command> {
        self.pending_send.retain(|c| c.id != command_id);
        self.store.confirm(command_id)
    }

    // ── Drain pending — main calls this to get commands to send ──
    pub fn drain_pending(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.pending_send)
    }

    // ── Drain failed — main calls this to surface failures ───
    //
    // Main logs them and updates shared state.
    // Confidence is NOT zeroed — a device can be unreachable for writes
    // while still reporting accurate state. These are distinct failure modes.
    pub fn drain_failed(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.failed_commands)
    }

    // ── Tick — called every second ───────────────────────────
    //
    // Checks timeouts and schedules retries.
    // Retry commands pushed to pending_send for main to resend.
    // Failed commands pushed to failed_commands for main to surface.
    pub fn tick(&mut self, now_ms: u64) {
        let timed_out = self.store.tick(now_ms);
        for item in timed_out {
            match item.action {
                TimeoutAction::Retry => {
                    info!(
                        command_id = %item.command.id,
                        device_id = %item.command.device_id,
                        retry = item.command.retry_count,
                        "command retrying"
                    );
                    self.pending_send.push(item.command);
                }
                TimeoutAction::Fail => {
                    error!(
                        command_id = %item.command.id,
                        device_id = %item.command.device_id,
                        "command permanently failed after max retries"
                    );
                    self.failed_commands.push(item.command);
                }
            }
        }
        self.store.cleanup(now_ms);
    }

    pub fn pending_count(&self) -> usize {
        self.store.pending().len()
    }

    pub fn all_commands(&self) -> Vec<&Command> {
        self.store.all()
    }
}