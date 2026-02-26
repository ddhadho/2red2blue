use crate::types::Command;

// Stub — full implementation in Milestone 5
pub struct ConflictResolver;

impl ConflictResolver {
    pub fn new() -> Self { Self }

    pub fn resolve(&self, candidates: Vec<Command>) -> Vec<Command> {
        // Pass everything through for now
        candidates
    }
}