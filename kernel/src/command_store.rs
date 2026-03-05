use std::collections::HashMap;
use tracing::{info, warn};
use crate::types::{Command, CommandStatus};

pub struct CommandStore {
    commands: HashMap<String, Command>,  // command_id → Command
    max_retries: u8,
    timeout_ms: u64,
}

impl CommandStore {
    pub fn new(max_retries: u8, timeout_ms: u64) -> Self {
        Self {
            commands: HashMap::new(),
            max_retries,
            timeout_ms,
        }
    }

    pub fn insert(&mut self, command: Command) {
        self.commands.insert(command.id.clone(), command);
    }

    pub fn mark_sent(&mut self, command_id: &str) {
        if let Some(cmd) = self.commands.get_mut(command_id) {
            cmd.status = CommandStatus::Sent;
        }
    }

    // Remove from store and return the confirmed command.
    // Main appends CommandConfirmed to WAL and updates shared state.
    pub fn confirm(&mut self, command_id: &str) -> Option<Command> {
        if let Some(mut cmd) = self.commands.remove(command_id) {
            cmd.status = CommandStatus::Confirmed;
            info!(
                command_id = %command_id,
                device_id = %cmd.device_id,
                "command confirmed"
            );
            Some(cmd)
        } else {
            None
        }
    }

    // Called every tick — returns commands that need action.
    // Retry: command is updated in place, returned for resend.
    // Fail: command is removed from store, returned for WAL write and confidence zero.
    pub fn tick(&mut self, now_ms: u64) -> Vec<TimedOutCommand> {
        let mut outcomes = vec![];
        let mut to_fail = vec![];

        for cmd in self.commands.values_mut() {
            if !matches!(cmd.status, CommandStatus::Sent) {
                continue;
            }

            let age = now_ms.saturating_sub(cmd.issued_at);
            if age <= self.timeout_ms {
                continue;
            }

            if cmd.retry_count < self.max_retries {
                cmd.retry_count += 1;
                cmd.status = CommandStatus::Pending;
                // Reset issued_at so next timeout window starts from now,
                // not from the original send time.
                cmd.issued_at = now_ms;
                warn!(
                    command_id = %cmd.id,
                    device_id = %cmd.device_id,
                    retry = cmd.retry_count,
                    "command timed out — retrying"
                );
                outcomes.push(TimedOutCommand {
                    command: cmd.clone(),
                    action: TimeoutAction::Retry,
                });
            } else {
                to_fail.push(cmd.id.clone());
            }
        }

        // Remove failed commands and return them separately —
        // can't remove from map while iterating over it.
        for id in to_fail {
            if let Some(mut cmd) = self.commands.remove(&id) {
                cmd.status = CommandStatus::Failed("max retries exceeded".to_string());
                warn!(
                    command_id = %cmd.id,
                    device_id = %cmd.device_id,
                    "command failed — max retries exceeded"
                );
                outcomes.push(TimedOutCommand {
                    command: cmd,
                    action: TimeoutAction::Fail,
                });
            }
        }

        outcomes
    }

    pub fn pending(&self) -> Vec<&Command> {
        self.commands.values()
            .filter(|c| matches!(c.status, CommandStatus::Pending))
            .collect()
    }

    pub fn all(&self) -> Vec<&Command> {
        self.commands.values().collect()
    }

    pub fn get(&self, id: &str) -> Option<&Command> {
        self.commands.get(id)
    }

    // Prune confirmed/failed commands older than 5 minutes.
    // Confirmed commands are removed on confirm() so this mainly
    // catches any stragglers and failed commands main didn't clean up.
    pub fn cleanup(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(5 * 60 * 1000);
        self.commands.retain(|_, cmd| {
            match &cmd.status {
                CommandStatus::Confirmed | CommandStatus::Failed(_) => {
                    cmd.issued_at > cutoff
                }
                _ => true,
            }
        });
    }
}

#[derive(Debug)]
pub struct TimedOutCommand {
    pub command: Command,
    pub action: TimeoutAction,
}

#[derive(Debug)]
pub enum TimeoutAction {
    Retry,
    Fail,
}