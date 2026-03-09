use std::time::SystemTime;
use tracing::{info, warn};
use crate::types::{
    Command, DeviceId,
};
use crate::state_engine::StateEngine;

// ── Report ───────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReconciliationReport {
    pub completed_at: u64,
    pub devices_evaluated: u32,
    pub mismatches_found: u32,
    pub commands_issued: u32,
    /// Devices with no actual state after the boot window —
    /// never reported, left alone.
    pub skipped_no_actual: u32,
    /// Devices below degraded threshold after the boot window —
    /// actual is stale, left for continuous reconciliation.
    pub skipped_low_confidence: u32,
    pub unreachable_devices: Vec<DeviceId>,
}

// ── Boot reconciliation ──────────────────────────────────────
//
// Called once before the event loop starts.
// By the time this runs:
//   - Desired state has been loaded from snapshot into state_engine
//   - WAL has been replayed into state_engine
//   - The 30-second boot window has elapsed
//
// This function diffs desired vs actual and returns correction commands.
// Main dispatches them through the normal resolver → dispatcher path.
// Main also writes the report to SharedState::last_reconciliation.

pub fn boot_reconcile(
    state_engine: &StateEngine,
    degraded_threshold: f32,
) -> (Vec<Command>, ReconciliationReport) {
    let mismatches = state_engine.diff();

    let mut commands = vec![];
    let mut skipped_no_actual = 0u32;
    let mut skipped_low_confidence = 0u32;
    let mut unreachable_devices = vec![];
    let mismatches_found = mismatches.len() as u32;

    for mismatch in mismatches {
        // Device has never reported actual state — skip.
        // Don't command a device we have no information about.
        if mismatch.actual.is_none() {
            skipped_no_actual += 1;
            unreachable_devices.push(mismatch.device_id.clone());
            warn!(
                device_id = %mismatch.device_id,
                attribute = %mismatch.attribute,
                "boot reconcile: no actual state — skipping"
            );
            continue;
        }

        // Confidence too low to trust actual — skip.
        // Continuous reconciliation will act once confidence recovers.
        if mismatch.confidence < degraded_threshold {
            skipped_low_confidence += 1;
            warn!(
                device_id = %mismatch.device_id,
                attribute = %mismatch.attribute,
                confidence = mismatch.confidence,
                "boot reconcile: low confidence — skipping"
            );
            continue;
        }

        // Actual is known and trustworthy — issue correction command.
        info!(
            device_id = %mismatch.device_id,
            attribute = %mismatch.attribute,
            desired = %mismatch.desired,
            actual = ?mismatch.actual,
            "boot reconcile: mismatch — issuing correction"
        );

        commands.push(Command::new(
            mismatch.device_id,
            mismatch.attribute,
            mismatch.desired,
            None,   // no rule_id — reconciler-originated
            255,    // highest priority — desired state enforcement beats rule commands
        ));
    }

    let devices_evaluated = state_engine.get_all().len() as u32;
    let commands_issued = commands.len() as u32;

    let report = ReconciliationReport {
        completed_at: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        devices_evaluated,
        mismatches_found,
        commands_issued,
        skipped_no_actual,
        skipped_low_confidence,
        unreachable_devices,
    };

    info!(
        devices_evaluated = report.devices_evaluated,
        mismatches_found = report.mismatches_found,
        commands_issued = report.commands_issued,
        skipped_no_actual = report.skipped_no_actual,
        skipped_low_confidence = report.skipped_low_confidence,
        "boot reconciliation complete"
    );

    (commands, report)
}

// ── Continuous reconciliation ─────────────────────────────────
//
// Called every tick in main after state_engine.tick and rule_engine.tick.
// Returns correction commands for any mismatches where actual is known
// and confidence is above the degraded threshold.
//
// Commands are reconciler-originated (no rule_id) with priority 255.
// They go through the normal resolver → dispatcher path.
//
// Skips devices with no actual state — they have never reported.
// Skips devices below degraded threshold — actual is stale.

pub fn continuous_reconcile(
    state_engine: &StateEngine,
    degraded_threshold: f32,
) -> Vec<Command> {
    state_engine
        .diff()
        .into_iter()
        .filter(|m| m.actual.is_some() && m.confidence >= degraded_threshold)
        .map(|m| {
            Command::new(
                m.device_id,
                m.attribute,
                m.desired,
                None,
                255,
            )
        })
        .collect()
}