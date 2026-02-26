use crate::types::Event;
use tracing::debug;

// In-memory WAL for Milestone 1
// Replaced with SQLite in Milestone 2
pub struct Wal {
    events: Vec<Event>,
}

impl Wal {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn append(&mut self, event: Event) -> u64 {
        let seq = event.sequence;
        debug!(sequence = seq, kind = ?event.kind, "WAL append");
        self.events.push(event);
        seq
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}