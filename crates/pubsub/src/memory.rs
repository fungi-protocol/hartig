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
