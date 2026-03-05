use std::collections::HashMap;
use tracing::{info, warn};
use crate::types::{Command, DeviceId, AttributeKey, RuleId};
use serde::{Serialize, Deserialize};

// ── Conflict record ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub winning_command_id: String,
    pub losing_command_id: String,
    pub device_id: DeviceId,
    pub attribute: AttributeKey,
    pub winning_rule: Option<RuleId>,
    pub losing_rule: Option<RuleId>,
    pub winning_priority: u8,
    pub losing_priority: u8,
    pub reason: ConflictReason,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictReason {
    LowerPriority,
    PriorityTie,  // tie broken by command ID — earlier ULID wins
}

// ── Resolved commands ────────────────────────────────────────

pub struct ResolvedCommands {
    pub winners: Vec<Command>,
    pub conflicts: Vec<ConflictRecord>,
}

// ── Resolver ─────────────────────────────────────────────────
//
// Stateless — no internal history, no priority map.
// Priority is read directly from Command::priority, stamped by
// the rule engine at production time.
// Main owns conflict history in SharedState.

pub struct ConflictResolver;

impl ConflictResolver {
    pub fn new() -> Self {
        Self
    }

    pub fn resolve(&self, candidates: Vec<Command>) -> ResolvedCommands {
        if candidates.is_empty() {
            return ResolvedCommands {
                winners: vec![],
                conflicts: vec![],
            };
        }

        // Group by (device_id, attribute) — same device same attribute is a conflict
        let mut groups: HashMap<(String, String), Vec<Command>> = HashMap::new();

        for cmd in candidates {
            let key = (cmd.device_id.0.clone(), cmd.attribute.0.clone());
            groups.entry(key).or_default().push(cmd);
        }

        let mut winners = vec![];
        let mut conflicts = vec![];
        let now_ms = now();

        for ((device_id, attribute), mut group) in groups {
            if group.len() == 1 {
                winners.push(group.remove(0));
                continue;
            }

            // Sort by priority descending, then by command ID ascending.
            // ULIDs are time-ordered — earlier command wins on tie.
            // This gives a stable, deterministic result without needing
            // external state.
            group.sort_by(|a, b| {
                b.priority.cmp(&a.priority)
                    .then_with(|| a.id.cmp(&b.id))
            });

            let winner = group.remove(0);

            info!(
                device_id = %device_id,
                attribute = %attribute,
                winner_rule = ?winner.rule_id,
                winner_priority = winner.priority,
                conflict_count = group.len(),
                "conflict resolved"
            );

            for loser in &group {
                let reason = if loser.priority == winner.priority {
                    ConflictReason::PriorityTie
                } else {
                    ConflictReason::LowerPriority
                };

                warn!(
                    device_id = %device_id,
                    attribute = %attribute,
                    losing_rule = ?loser.rule_id,
                    losing_priority = loser.priority,
                    reason = ?reason,
                    "command lost conflict resolution"
                );

                conflicts.push(ConflictRecord {
                    winning_command_id: winner.id.clone(),
                    losing_command_id: loser.id.clone(),
                    device_id: DeviceId(device_id.clone()),
                    attribute: AttributeKey(attribute.clone()),
                    winning_rule: winner.rule_id.clone(),
                    losing_rule: loser.rule_id.clone(),
                    winning_priority: winner.priority,
                    losing_priority: loser.priority,
                    reason,
                    timestamp: now_ms,
                });
            }

            winners.push(winner);
        }

        ResolvedCommands { winners, conflicts }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}