use crate::types::{Seq, TopicId};

/// The storage backend itself failed, not the caller's input.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("storage backend failed: {0}")]
    Backend(String),
}

/// Messages read from a topic, plus the topic's length at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadResult {
    /// Messages in position order, starting at the requested position.
    pub messages: Vec<Vec<u8>>,
    /// Total messages in the topic, taken in the same atomic step as `messages`.
    pub total_count: Seq,
}

/// Per-topic message logs. Every backend must guarantee:
/// - Messages are append-only and kept in arrival order.
/// - Positions start at 0 per topic and never shift or get reused.
/// - Methods are safe to call concurrently; the backend does its own locking.
/// - Message size is not checked here; the HTTP layer enforces it.
pub trait TopicStore: Send + Sync {
    /// Append `message` to `topic`, taking ownership of it, and return its position.
    /// - The first append creates the topic; until a message is stored, reading it looks the same as reading an unknown topic.
    /// - A duplicate (same bytes, same topic) stores nothing and returns the original position.
    fn append(&self, topic: &TopicId, message: Vec<u8>) -> Result<Seq, StoreError>;

    /// Read up to `max` messages from position `start` (`None` reads to the end).
    /// - Reading past the end, or an unknown topic, returns no messages and is not an error.
    /// - Reading never creates a topic.
    fn read(
        &self,
        topic: &TopicId,
        start: Seq,
        max: Option<usize>,
    ) -> Result<ReadResult, StoreError>;
}
