use crate::store::{ReadResult, StoreError, TopicStore};
use crate::types::{Seq, TopicId};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Default)]
struct LogData {
    messages: Vec<Vec<u8>>,
    dupe_index: HashMap<[u8; 32], Seq>,
}

#[derive(Default)]
struct TopicLog {
    data: RwLock<LogData>,
}

impl TopicLog {
    /// Adds `message` unless it's already here, and returns its position either way.
    fn append(&self, message: Vec<u8>) -> Seq {
        let message_hash = hash_message(&message);
        let mut data = self.data.write().expect("topic lock poisoned");

        if let Some(&seq) = data.dupe_index.get(&message_hash) {
            return seq;
        }

        let seq = data.messages.len() as Seq;
        data.messages.push(message);
        data.dupe_index.insert(message_hash, seq);
        seq
    }

    fn read(&self, start: Seq, max: Option<usize>) -> ReadResult {
        let data = self.data.read().expect("topic lock poisoned");

        let len = data.messages.len();
        let total_count = len as Seq;

        let start = match usize::try_from(start) {
            Ok(start) if start < len => start,
            _ => {
                return ReadResult {
                    messages: Vec::new(),
                    total_count,
                };
            }
        };

        let end = match max {
            Some(max) => len.min(start.saturating_add(max)),
            None => len,
        };

        ReadResult {
            messages: data.messages[start..end].to_vec(),
            total_count,
        }
    }
}

#[derive(Default)]
pub struct MemoryStore {
    topics: RwLock<HashMap<TopicId, Arc<TopicLog>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn hash_message(message: &[u8]) -> [u8; 32] {
    Sha256::digest(message).into()
}

impl TopicStore for MemoryStore {
    fn append(&self, topic: &TopicId, message: Vec<u8>) -> Result<Seq, StoreError> {
        let existing = self
            .topics
            .read()
            .expect("topics lock poisoned")
            .get(topic)
            .cloned();
        let log = match existing {
            Some(log) => log,
            None => {
                let mut topics = self.topics.write().expect("topics lock poisoned");
                Arc::clone(topics.entry(*topic).or_default())
            }
        };
        Ok(log.append(message))
    }

    fn read(
        &self,
        topic: &TopicId,
        start: Seq,
        max: Option<usize>,
    ) -> Result<ReadResult, StoreError> {
        // Reading must never create a topic.
        let Some(log) = self
            .topics
            .read()
            .expect("topics lock poisoned")
            .get(topic)
            .cloned()
        else {
            return Ok(ReadResult {
                messages: Vec::new(),
                total_count: 0,
            });
        };

        Ok(log.read(start, max))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::MemoryStore;
    use crate::store::{TopicStore, store_contract};
    use crate::types::TopicId;

    #[test]
    fn positions_increase_by_one() {
        store_contract::positions_increase_by_one(&MemoryStore::new());
    }

    #[test]
    fn topics_are_isolated() {
        store_contract::topics_are_isolated(&MemoryStore::new());
    }

    #[test]
    fn duplicate_is_stored_once() {
        store_contract::duplicate_is_stored_once(&MemoryStore::new());
    }

    #[test]
    fn duplicates_are_per_topic() {
        store_contract::duplicates_are_per_topic(&MemoryStore::new());
    }

    #[test]
    fn read_from_position_returns_newer() {
        store_contract::read_from_position_returns_newer(&MemoryStore::new());
    }

    #[test]
    fn read_respects_max() {
        store_contract::read_respects_max(&MemoryStore::new());
    }

    #[test]
    fn read_past_end_returns_nothing() {
        store_contract::read_past_end_returns_nothing(&MemoryStore::new());
    }

    #[test]
    fn read_unknown_topic_returns_nothing() {
        store_contract::read_unknown_topic_returns_nothing(&MemoryStore::new());
    }

    #[test]
    fn read_unknown_topic_creates_nothing() {
        let store = MemoryStore::new();
        store.read(&TopicId::new([1; 16]), 0, None).unwrap();
        assert!(store.topics.read().unwrap().is_empty());
    }

    #[test]
    fn fan_out() {
        store_contract::fan_out(&MemoryStore::new());
    }

    #[test]
    fn concurrent_appends_get_unique_gap_free_positions() {
        store_contract::concurrent_appends_get_unique_gap_free_positions(&MemoryStore::new());
    }
}
